package org.rstorrent.bootstrap.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Sort
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.rstorrent.bootstrap.ProductState
import org.rstorrent.session.uniffi.TorrentOperationalState
import org.rstorrent.session.uniffi.TorrentView

@OptIn(ExperimentalFoundationApi::class, ExperimentalMaterial3Api::class)
@Composable
internal fun LibraryScreen(
    state: ProductState,
    notificationsGranted: Boolean,
    onRequestNotifications: () -> Unit,
    onSelectStorage: () -> Unit,
    onOpenTorrent: (String) -> Unit,
    onAddMagnet: (String, Boolean) -> Unit,
    onBrowseTorrent: (Boolean) -> Unit,
    onPause: (String) -> Unit,
    onResume: (String) -> Unit,
    onMoveTop: (String) -> Unit,
    onMoveBottom: (String) -> Unit,
    onArchive: (String) -> Unit,
    onRemove: (Set<String>) -> Unit,
    onSpeed: () -> Unit,
    onDht: () -> Unit,
    onLogs: () -> Unit,
    onSettings: () -> Unit,
    onShutdown: () -> Unit,
) {
    var filterName by rememberSaveable { mutableStateOf(LibraryFilter.ALL.name) }
    var sortName by rememberSaveable { mutableStateOf(LibrarySort.STABLE.name) }
    var addOpen by rememberSaveable { mutableStateOf(false) }
    var sortOpen by remember { mutableStateOf(false) }
    var overflowOpen by remember { mutableStateOf(false) }
    var selection by rememberSaveable { mutableStateOf(emptySet<String>()) }
    val filter = LibraryFilter.valueOf(filterName)
    val sort = LibrarySort.valueOf(sortName)
    val torrents = filteredAndSortedTorrents(state.torrents.values, filter, sort)

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        RstorrentLogo(Modifier.size(40.dp))
                        Spacer(Modifier.size(10.dp))
                        Column {
                            Text("RSTorrent", fontWeight = FontWeight.SemiBold)
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Surface(
                                    modifier = Modifier.size(8.dp),
                                    shape = CircleShape,
                                    color =
                                        if (state.ready) {
                                            Color(0xFF2E7D32)
                                        } else {
                                            MaterialTheme.colorScheme.outline
                                        },
                                ) {}
                                Spacer(Modifier.size(6.dp))
                                Text(
                                    if (state.ready) "Live" else "Connecting",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                },
                actions = {
                    Box {
                        IconButton(
                            onClick = { sortOpen = true },
                            modifier = Modifier.semantics { contentDescription = "Sort torrents" },
                        ) {
                            Icon(Icons.AutoMirrored.Outlined.Sort, contentDescription = null)
                        }
                        DropdownMenu(expanded = sortOpen, onDismissRequest = { sortOpen = false }) {
                            LibrarySort.entries.forEach { candidate ->
                                DropdownMenuItem(
                                    text = { Text(candidate.label) },
                                    onClick = {
                                        sortName = candidate.name
                                        sortOpen = false
                                    },
                                    leadingIcon = {
                                        if (candidate == sort) Text("✓")
                                    },
                                )
                            }
                        }
                    }
                    Box {
                        IconButton(
                            onClick = { overflowOpen = true },
                            modifier = Modifier.semantics { contentDescription = "More options" },
                        ) {
                            Icon(Icons.Outlined.MoreVert, contentDescription = null)
                        }
                        LibraryOverflowMenu(
                            expanded = overflowOpen,
                            onDismiss = { overflowOpen = false },
                            onPauseAll = {
                                state.torrents.values.forEach { onPause(it.torrentId) }
                            },
                            onResumeAll = {
                                state.torrents.values.forEach { onResume(it.torrentId) }
                            },
                            onSelectStorage = onSelectStorage,
                            onSpeed = onSpeed,
                            onDht = onDht,
                            onLogs = onLogs,
                            onSettings = onSettings,
                            onShutdown = onShutdown,
                        )
                    }
                },
            )
        },
        floatingActionButton = {
            if (selection.isEmpty()) {
                ExtendedFloatingActionButton(
                    onClick = { addOpen = true },
                    icon = { Icon(Icons.Default.Add, contentDescription = null) },
                    text = { Text("Add") },
                    modifier = Modifier.semantics { contentDescription = "Add torrent" },
                )
            }
        },
        bottomBar = {
            if (selection.isNotEmpty()) {
                SelectionActions(
                    count = selection.size,
                    onPause = { selection.forEach(onPause) },
                    onResume = { selection.forEach(onResume) },
                    onMoveTop = { selection.forEach(onMoveTop) },
                    onMoveBottom = { selection.forEach(onMoveBottom) },
                    onArchive = { selection.forEach(onArchive) },
                    onRemove = { onRemove(selection) },
                    onClear = { selection = emptySet() },
                )
            }
        },
    ) { padding ->
        Column(modifier = Modifier.fillMaxSize().padding(padding)) {
            ScrollableTabRow(selectedTabIndex = filter.ordinal, edgePadding = 12.dp) {
                LibraryFilter.entries.forEach { candidate ->
                    Tab(
                        selected = candidate == filter,
                        onClick = { filterName = candidate.name },
                        text = { Text(candidate.label) },
                    )
                }
            }
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(16.dp, 12.dp, 16.dp, 96.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                if (!notificationsGranted) {
                    item("notifications") {
                        SetupCard(
                            title = "Enable notifications",
                            detail =
                                "RSTorrent uses a foreground notification while torrents are active.",
                            action = "Enable",
                            onAction = onRequestNotifications,
                        )
                    }
                }
                if (!state.storageRootReady) {
                    item("storage") {
                        SetupCard(
                            title = "Choose a download folder",
                            detail =
                                state.error
                                    ?: "Select a folder before adding or resuming downloads.",
                            action = if (state.storageRootLabel == null) "Select folder" else "Repair",
                            onAction = onSelectStorage,
                        )
                    }
                }
                state.error?.takeIf { state.storageRootReady }?.let { error ->
                    item("error") {
                        Card(
                            modifier = Modifier.fillMaxWidth().widthIn(max = 720.dp),
                            colors = CardDefaults.cardColors(MaterialTheme.colorScheme.errorContainer),
                        ) {
                            Text(
                                error,
                                modifier = Modifier.padding(16.dp),
                                color = MaterialTheme.colorScheme.onErrorContainer,
                            )
                        }
                    }
                }
                if (torrents.isEmpty()) {
                    item("empty") { EmptyLibrary(filter) }
                }
                items(torrents, key = TorrentView::torrentId) { torrent ->
                    TorrentCard(
                        torrent = torrent,
                        selected = torrent.torrentId in selection,
                        selectionMode = selection.isNotEmpty(),
                        onClick = {
                            if (selection.isEmpty()) {
                                onOpenTorrent(torrent.torrentId)
                            } else {
                                selection = selection.toggle(torrent.torrentId)
                            }
                        },
                        onLongClick = { selection = selection.toggle(torrent.torrentId) },
                        onPause = { onPause(torrent.torrentId) },
                        onResume = { onResume(torrent.torrentId) },
                    )
                }
            }
        }
    }

    if (addOpen) {
        AddTorrentDialog(
            enabled = state.ready && state.storageRootReady,
            onDismiss = { addOpen = false },
            onAddMagnet = {
                onAddMagnet(it.first, it.second)
                addOpen = false
            },
            onBrowse = { start ->
                addOpen = false
                onBrowseTorrent(start)
            },
        )
    }
}

@Composable
private fun LibraryOverflowMenu(
    expanded: Boolean,
    onDismiss: () -> Unit,
    onPauseAll: () -> Unit,
    onResumeAll: () -> Unit,
    onSelectStorage: () -> Unit,
    onSpeed: () -> Unit,
    onDht: () -> Unit,
    onLogs: () -> Unit,
    onSettings: () -> Unit,
    onShutdown: () -> Unit,
) {
    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss) {
        DropdownMenuItem(text = { Text("Pause All") }, onClick = { onDismiss(); onPauseAll() })
        DropdownMenuItem(text = { Text("Resume All") }, onClick = { onDismiss(); onResumeAll() })
        DropdownMenuItem(
            text = { Text("Add Download Folder") },
            leadingIcon = { Icon(Icons.Outlined.Folder, contentDescription = null) },
            onClick = { onDismiss(); onSelectStorage() },
        )
        HorizontalDivider()
        DropdownMenuItem(text = { Text("Speed") }, onClick = { onDismiss(); onSpeed() })
        DropdownMenuItem(text = { Text("DHT Info") }, onClick = { onDismiss(); onDht() })
        DropdownMenuItem(text = { Text("Logs") }, onClick = { onDismiss(); onLogs() })
        DropdownMenuItem(text = { Text("Settings") }, onClick = { onDismiss(); onSettings() })
        DropdownMenuItem(text = { Text("Shutdown") }, onClick = { onDismiss(); onShutdown() })
    }
}

@Composable
private fun SetupCard(
    title: String,
    detail: String,
    action: String,
    onAction: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth().widthIn(max = 720.dp),
        colors = CardDefaults.cardColors(MaterialTheme.colorScheme.secondaryContainer),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(4.dp))
            Text(detail, color = MaterialTheme.colorScheme.onSecondaryContainer)
            Spacer(Modifier.height(12.dp))
            Button(onClick = onAction) { Text(action) }
        }
    }
}

@Composable
private fun EmptyLibrary(filter: LibraryFilter) {
    Column(
        modifier = Modifier.fillMaxWidth().padding(vertical = 48.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            Icons.Outlined.Folder,
            contentDescription = null,
            modifier = Modifier.size(48.dp),
            tint = MaterialTheme.colorScheme.outline,
        )
        Spacer(Modifier.height(12.dp))
        Text(
            if (filter == LibraryFilter.ALL) "No torrents yet" else "No ${filter.label.lowercase()} torrents",
            style = MaterialTheme.typography.titleMedium,
        )
        Text(
            if (filter == LibraryFilter.ALL) "Tap Add to paste a magnet or choose a .torrent file."
            else "Try another library filter.",
            modifier = Modifier.padding(top = 4.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun TorrentCard(
    torrent: TorrentView,
    selected: Boolean,
    selectionMode: Boolean,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
    onPause: () -> Unit,
    onResume: () -> Unit,
) {
    val progress = torrentProgress(torrent)
    val paused = torrent.operationalState == TorrentOperationalState.PAUSED
    Card(
        modifier =
            Modifier.fillMaxWidth().widthIn(max = 720.dp)
                .combinedClickable(onClick = onClick, onLongClick = onLongClick),
        colors =
            CardDefaults.cardColors(
                if (selected) MaterialTheme.colorScheme.primaryContainer
                else MaterialTheme.colorScheme.surfaceContainer,
            ),
        shape = RoundedCornerShape(16.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(
                onClick = if (paused) onResume else onPause,
                enabled = !selectionMode,
                modifier =
                    Modifier.clip(CircleShape)
                        .semantics { contentDescription = if (paused) "Resume" else "Pause" },
            ) {
                Icon(
                    if (paused) Icons.Default.PlayArrow else Icons.Default.Pause,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                )
            }
            Column(Modifier.weight(1f)) {
                Text(
                    torrent.displayName ?: torrent.torrentId,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    fontWeight = FontWeight.SemiBold,
                )
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(
                        operationalLabel(torrent.operationalState) +
                            (torrent.downloadQueuePosition?.let { " · Queue $it" } ?: ""),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(percentLabel(progress), style = MaterialTheme.typography.labelMedium)
                }
                Spacer(Modifier.height(6.dp))
                LinearProgressIndicator(
                    progress = { progress },
                    modifier = Modifier.fillMaxWidth().height(6.dp).clip(CircleShape),
                )
                Spacer(Modifier.height(7.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("↓ ${formatRate(torrent.payloadDownloadRateBytes)}")
                    Text("ETA ${torrentEta(torrent)}")
                }
            }
        }
    }
}

@Composable
private fun SelectionActions(
    count: Int,
    onPause: () -> Unit,
    onResume: () -> Unit,
    onMoveTop: () -> Unit,
    onMoveBottom: () -> Unit,
    onArchive: () -> Unit,
    onRemove: () -> Unit,
    onClear: () -> Unit,
) {
    Surface(shadowElevation = 8.dp) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("$count selected", modifier = Modifier.weight(1f), fontWeight = FontWeight.SemiBold)
                TextButton(onClick = onPause) { Text("Pause") }
                TextButton(onClick = onResume) { Text("Resume") }
                TextButton(onClick = onClear) { Text("Done") }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = onMoveTop) { Text("Top") }
                TextButton(onClick = onMoveBottom) { Text("Bottom") }
                TextButton(onClick = onArchive) { Text("Archive/restore") }
                TextButton(onClick = onRemove) { Text("Remove") }
            }
        }
    }
}

@Composable
private fun AddTorrentDialog(
    enabled: Boolean,
    onDismiss: () -> Unit,
    onAddMagnet: (Pair<String, Boolean>) -> Unit,
    onBrowse: (Boolean) -> Unit,
) {
    var magnet by rememberSaveable { mutableStateOf("") }
    var startContent by rememberSaveable { mutableStateOf(true) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add torrent") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(
                    value = magnet,
                    onValueChange = { magnet = it },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("Magnet link") },
                    minLines = 3,
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(checked = startContent, onCheckedChange = { startContent = it })
                    Text("Start downloading immediately")
                }
                OutlinedButton(
                    onClick = { onBrowse(startContent) },
                    enabled = enabled,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Browse .torrent file")
                }
                if (!enabled) {
                    Text(
                        "Choose an available download folder first.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        },
        confirmButton = {
            Button(
                onClick = { onAddMagnet(magnet.trim() to startContent) },
                enabled = enabled && magnet.isNotBlank(),
            ) { Text("Add") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
internal fun RstorrentLogo(modifier: Modifier = Modifier) {
    androidx.compose.foundation.Canvas(modifier) {
        drawCircle(color = Color(0xFF006A6A))
        val stroke = Stroke(width = size.minDimension * 0.1f)
        val x = size.width / 2f
        val top = size.height * 0.23f
        val bottom = size.height * 0.68f
        drawLine(Color.White, start = androidx.compose.ui.geometry.Offset(x, top), end = androidx.compose.ui.geometry.Offset(x, bottom), strokeWidth = stroke.width)
        drawLine(Color.White, start = androidx.compose.ui.geometry.Offset(x, bottom), end = androidx.compose.ui.geometry.Offset(size.width * 0.32f, size.height * 0.5f), strokeWidth = stroke.width)
        drawLine(Color.White, start = androidx.compose.ui.geometry.Offset(x, bottom), end = androidx.compose.ui.geometry.Offset(size.width * 0.68f, size.height * 0.5f), strokeWidth = stroke.width)
        drawLine(Color.White, start = androidx.compose.ui.geometry.Offset(size.width * 0.3f, size.height * 0.8f), end = androidx.compose.ui.geometry.Offset(size.width * 0.7f, size.height * 0.8f), strokeWidth = stroke.width)
    }
}

private fun Set<String>.toggle(value: String): Set<String> =
    if (value in this) this - value else this + value
