@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package org.rstorrent.bootstrap.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.BatterySaver
import androidx.compose.material.icons.outlined.Folder
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material.icons.outlined.NetworkCheck
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Speed
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.Divider
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import kotlinx.coroutines.launch
import org.rstorrent.bootstrap.ProductEngineService
import org.rstorrent.bootstrap.ProductState
import org.rstorrent.bootstrap.GlobalPresentation
import org.rstorrent.bootstrap.TorrentPresentation
import org.rstorrent.session.uniffi.DiagnosticEvent
import org.rstorrent.session.uniffi.DiagnosticCategory
import org.rstorrent.session.uniffi.DiagnosticProfile
import org.rstorrent.session.uniffi.DiagnosticSeverity
import org.rstorrent.session.uniffi.RemovalDataPolicy
import org.rstorrent.session.uniffi.TorrentOperationalState
import org.rstorrent.session.uniffi.TorrentView

@Composable
fun ProductApp(
    service: ProductEngineService?,
    onSelectStorage: () -> Unit,
    onBrowseTorrent: (Boolean) -> Unit,
    notificationsGranted: Boolean,
    onRequestNotifications: () -> Unit,
    onOpenNotificationSettings: () -> Unit,
    themeMode: ProductThemeMode,
    dynamicColor: Boolean,
    onThemeMode: (ProductThemeMode) -> Unit,
    onDynamicColor: (Boolean) -> Unit,
) {
    RstorrentTheme(mode = themeMode, dynamicColor = dynamicColor) {
        val state =
            if (service == null) {
                ProductState()
            } else {
                val collected by service.state.collectAsStateWithLifecycle()
                collected
            }
        Surface(modifier = Modifier.fillMaxSize()) {
            ProductNavHost(
                state = state,
                service = service,
                onSelectStorage = onSelectStorage,
                onBrowseTorrent = onBrowseTorrent,
                notificationsGranted = notificationsGranted,
                onRequestNotifications = onRequestNotifications,
                onOpenNotificationSettings = onOpenNotificationSettings,
                themeMode = themeMode,
                dynamicColor = dynamicColor,
                onThemeMode = onThemeMode,
                onDynamicColor = onDynamicColor,
            )
        }
    }
}

@Composable
private fun ProductNavHost(
    state: ProductState,
    service: ProductEngineService?,
    onSelectStorage: () -> Unit,
    onBrowseTorrent: (Boolean) -> Unit,
    notificationsGranted: Boolean,
    onRequestNotifications: () -> Unit,
    onOpenNotificationSettings: () -> Unit,
    themeMode: ProductThemeMode,
    dynamicColor: Boolean,
    onThemeMode: (ProductThemeMode) -> Unit,
    onDynamicColor: (Boolean) -> Unit,
) {
    val navController = rememberNavController()
    var removeTargets by remember { mutableStateOf(emptySet<String>()) }
    NavHost(navController = navController, startDestination = ProductRoutes.LIBRARY) {
        composable(ProductRoutes.LIBRARY) {
            LibraryScreen(
                state = state,
                notificationsGranted = notificationsGranted,
                onRequestNotifications = onRequestNotifications,
                onSelectStorage = onSelectStorage,
                onOpenTorrent = { navController.navigate(ProductRoutes.detail(it)) },
                onAddMagnet = { magnet, start ->
                    service?.addMagnet(magnet, startContent = start)
                },
                onBrowseTorrent = onBrowseTorrent,
                onPause = { service?.pause(it) },
                onResume = { service?.resume(it) },
                onMoveTop = { service?.moveDownloadToTop(it) },
                onMoveBottom = { service?.moveDownloadToBottom(it) },
                onArchive = { id ->
                    if (state.torrents[id]?.archived == true) service?.restoreArchive(id)
                    else service?.archive(id)
                },
                onRemove = { removeTargets = it },
                onSpeed = { navController.navigate(ProductRoutes.SPEED) },
                onDht = { navController.navigate(ProductRoutes.DHT) },
                onLogs = { navController.navigate(ProductRoutes.LOGS) },
                onSettings = { navController.navigate(ProductRoutes.SETTINGS) },
                onShutdown = { service?.shutdownFromUi() },
            )
        }
        composable(ProductRoutes.DETAIL) { entry ->
            val torrentId = requireNotNull(entry.arguments?.getString("torrentId"))
            val torrent = state.torrents[torrentId]
            DisposableEffect(torrentId) {
                service?.selectTorrent(torrentId)
                onDispose { service?.clearTorrentPresentation(torrentId) }
            }
            TorrentDetailScreen(
                torrent = torrent,
                state = state,
                onBack = navController::popBackStack,
                onPause = { service?.pause(torrentId) },
                onResume = { service?.resume(torrentId) },
                onForceRecheck = { service?.forceRecheck(torrentId) },
                onMoveTop = { service?.moveDownloadToTop(torrentId) },
                onMoveBottom = { service?.moveDownloadToBottom(torrentId) },
                onArchive = { service?.archive(torrentId) },
                onRestore = { service?.restoreArchive(torrentId) },
                onRemove = { removeTargets = setOf(torrentId) },
                onCopyMagnet = { service?.copyMagnet(torrentId) },
                onSpeed = { navController.navigate(ProductRoutes.SPEED) },
                onDht = { navController.navigate(ProductRoutes.DHT) },
                onLogs = { navController.navigate(ProductRoutes.LOGS) },
                onSettings = { navController.navigate(ProductRoutes.SETTINGS) },
                onPresent = { service?.presentTorrent(torrentId, it) },
                onSetFileWanted = { file, wanted ->
                    service?.setFileWanted(torrentId, file.fileIndex, wanted)
                },
                onDownloadFile = { service?.downloadFileNow(torrentId, it.fileIndex) },
                onOpenFile = { file ->
                    torrent?.displayName?.let { service?.openCompletedFile(it, file) }
                },
                onFilePage = {
                    service?.presentCatalogPage(torrentId, TorrentPresentation.FILES, it)
                },
                onTrackerPage = {
                    service?.presentCatalogPage(torrentId, TorrentPresentation.TRACKERS, it)
                },
            )
        }
        composable(ProductRoutes.SPEED) {
            DisposableEffect(service) {
                service?.presentGlobal(GlobalPresentation.SPEED)
                onDispose { service?.presentGlobal(GlobalPresentation.NONE) }
            }
            SpeedScreen(state.speed, navController::popBackStack)
        }
        composable(ProductRoutes.DHT) {
            DisposableEffect(service) {
                service?.presentGlobal(GlobalPresentation.DHT)
                onDispose { service?.presentGlobal(GlobalPresentation.NONE) }
            }
            DhtScreen(state.dht, navController::popBackStack)
        }
        composable(ProductRoutes.LOGS) {
            LogsShell(
                events = state.diagnostics,
                sourceEvicted = state.diagnosticSourceEvicted,
                localEvicted = state.diagnosticLocalEvicted,
                resets = state.diagnosticResets,
                selectedTorrent = state.selectedTorrent,
                service = service,
                onBack = navController::popBackStack,
            )
        }
        composable(ProductRoutes.SETTINGS) {
            SettingsHub(navController)
        }
        composable(ProductRoutes.SETTINGS_STORAGE) {
            SettingsPage("Storage", navController::popBackStack) {
                SettingAction(
                    title = state.storageRootLabel ?: "Download folder",
                    detail = if (state.storageRootReady) "Available" else "Unavailable or not selected",
                    onClick = onSelectStorage,
                    action = if (state.storageRootReady) "Change" else "Select",
                )
                state.storage?.roots.orEmpty().forEach { root ->
                    ReadOnlySettingsRow(
                        root.label + if (root.rootId == state.storage?.defaultRoot) " (default)" else "",
                        root.availability.name.lowercase() +
                            (root.displayPath?.let { " · $it" } ?: " · Android document provider"),
                    )
                }
            }
        }
        composable(ProductRoutes.SETTINGS_SPEED) {
            SettingsPage("Speed & Connection Limits", navController::popBackStack) {
                val settings = state.clientSettings
                if (settings == null) {
                    Text("Settings are loading…", modifier = Modifier.padding(16.dp))
                } else {
                    ConnectionLimitsSettings(
                        settings,
                        onPeerConnections = { value ->
                            service?.updateClientSettings { it.copy(peerConnectionLimit = value) }
                        },
                        onUploadSlots = { value ->
                            service?.updateClientSettings { it.copy(uploadSlots = value) }
                        },
                        onActiveDownloads = { value ->
                            service?.updateClientSettings { it.copy(activeDownloads = value) }
                        },
                    )
                }
            }
        }
        composable(ProductRoutes.SETTINGS_NOTIFICATIONS) {
            SettingsPage("Notifications", navController::popBackStack) {
                SettingAction(
                    title = if (notificationsGranted) "Notifications enabled" else "Notifications disabled",
                    detail = "Foreground-service status is managed by Android.",
                    onClick = if (notificationsGranted) onOpenNotificationSettings else onRequestNotifications,
                    action = if (notificationsGranted) "Manage" else "Enable",
                )
                UnavailableSetting("Completion notifications")
            }
        }
        composable(ProductRoutes.SETTINGS_NETWORK) {
            SettingsPage("Network & Privacy", navController::popBackStack) {
                state.clientSettings?.let { settings ->
                    NetworkSettings(
                        settings,
                        onListener = { enabled ->
                            service?.updateClientSettings {
                                it.copy(
                                    listener =
                                        if (enabled) {
                                            org.rstorrent.session.uniffi.ListenerPolicy.AutomaticLocalNetwork
                                        } else {
                                            org.rstorrent.session.uniffi.ListenerPolicy.Disabled
                                        },
                                )
                            }
                        },
                        onPortMapping = { enabled ->
                            service?.updateClientSettings {
                                it.copy(
                                    portMapping =
                                        if (enabled) {
                                            org.rstorrent.session.uniffi.PortMappingPolicy.UPNP
                                        } else {
                                            org.rstorrent.session.uniffi.PortMappingPolicy.DISABLED
                                        },
                                )
                            }
                        },
                        onIpv6 = { enabled ->
                            service?.updateClientSettings { it.copy(ipv6Enabled = enabled) }
                        },
                        onEncryption = { policy ->
                            service?.updateClientSettings { it.copy(encryption = policy) }
                        },
                    )
                } ?: Text("Settings are loading…", modifier = Modifier.padding(16.dp))
            }
        }
        composable(ProductRoutes.SETTINGS_POWER) {
            SettingsPage("Power Management", navController::popBackStack) {
                ReadOnlySetting(
                    "Foreground operation",
                    "RSTorrent holds Android power and Wi-Fi locks only while transfer work is active.",
                )
                UnavailableSetting("Battery policy")
            }
        }
        composable(ProductRoutes.SETTINGS_ADVANCED) {
            SettingsPage("Advanced", navController::popBackStack) {
                Text("Theme", modifier = Modifier.padding(16.dp), fontWeight = FontWeight.SemiBold)
                ProductThemeMode.entries.forEach { mode ->
                    ListItem(
                        headlineContent = { Text(mode.name.lowercase().replaceFirstChar(Char::titlecase)) },
                        leadingContent = {
                            RadioButton(selected = themeMode == mode, onClick = { onThemeMode(mode) })
                        },
                        modifier = Modifier.semantics { role = Role.RadioButton },
                    )
                }
                ListItem(
                    headlineContent = { Text("Use system colors") },
                    supportingContent = { Text("Available on Android 12 and later") },
                    trailingContent = {
                        Switch(checked = dynamicColor, onCheckedChange = onDynamicColor)
                    },
                )
                Divider()
                UnavailableSetting("Search plugins")
                UnavailableSetting("Reset engine settings")
            }
        }
    }
    if (removeTargets.isNotEmpty()) {
        RemoveDialog(
            count = removeTargets.size,
            onDismiss = { removeTargets = emptySet() },
            onKeep = {
                removeTargets.forEach { service?.removeTorrent(it, RemovalDataPolicy.KEEP) }
                removeTargets = emptySet()
            },
            onDelete = {
                removeTargets.forEach { service?.removeTorrent(it, RemovalDataPolicy.DELETE_MANAGED) }
                removeTargets = emptySet()
            },
        )
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun TorrentDetailScreen(
    torrent: TorrentView?,
    state: ProductState,
    onBack: () -> Unit,
    onPause: () -> Unit,
    onResume: () -> Unit,
    onForceRecheck: () -> Unit,
    onMoveTop: () -> Unit,
    onMoveBottom: () -> Unit,
    onArchive: () -> Unit,
    onRestore: () -> Unit,
    onRemove: () -> Unit,
    onCopyMagnet: () -> Unit,
    onSpeed: () -> Unit,
    onDht: () -> Unit,
    onLogs: () -> Unit,
    onSettings: () -> Unit,
    onPresent: (TorrentPresentation) -> Unit,
    onSetFileWanted: (org.rstorrent.session.uniffi.FileView, Boolean) -> Unit,
    onDownloadFile: (org.rstorrent.session.uniffi.FileView) -> Unit,
    onOpenFile: (org.rstorrent.session.uniffi.FileView) -> Unit,
    onFilePage: (UInt) -> Unit,
    onTrackerPage: (UInt) -> Unit,
) {
    var overflow by remember { mutableStateOf(false) }
    val pager = rememberPagerState(pageCount = { TorrentDetailTab.entries.size })
    val scope = rememberCoroutineScope()
    LaunchedEffect(pager.currentPage) {
        val presentation =
            when (TorrentDetailTab.entries[pager.currentPage]) {
                TorrentDetailTab.DETAILS,
                TorrentDetailTab.STATUS,
                -> TorrentPresentation.SUMMARY
                TorrentDetailTab.FILES -> TorrentPresentation.FILES
                TorrentDetailTab.TRACKERS -> TorrentPresentation.TRACKERS
                TorrentDetailTab.PEERS -> TorrentPresentation.PEERS
                TorrentDetailTab.PIECES -> TorrentPresentation.PIECES
            }
        onPresent(presentation)
    }
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                    }
                },
                title = {
                    Text(
                        torrent?.displayName ?: torrent?.torrentId ?: "Torrent",
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                actions = {
                    val paused = torrent?.operationalState == TorrentOperationalState.PAUSED
                    IconButton(onClick = if (paused) onResume else onPause, enabled = torrent != null) {
                        Icon(
                            if (paused) Icons.Default.PlayArrow else Icons.Default.Pause,
                            contentDescription = if (paused) "Resume" else "Pause",
                        )
                    }
                    Box {
                        IconButton(onClick = { overflow = true }) {
                            Icon(Icons.Outlined.MoreVert, contentDescription = "More options")
                        }
                        DropdownMenu(expanded = overflow, onDismissRequest = { overflow = false }) {
                            DropdownMenuItem(
                                text = { Text("Force recheck") },
                                enabled = torrent?.forceRecheckAvailable == true,
                                onClick = { overflow = false; onForceRecheck() },
                            )
                            DropdownMenuItem(text = { Text("Move to top") }, onClick = { overflow = false; onMoveTop() })
                            DropdownMenuItem(text = { Text("Move to bottom") }, onClick = { overflow = false; onMoveBottom() })
                            DropdownMenuItem(
                                text = { Text(if (torrent?.archived == true) "Restore archive" else "Archive") },
                                onClick = { overflow = false; if (torrent?.archived == true) onRestore() else onArchive() },
                            )
                            DropdownMenuItem(text = { Text("Remove torrent") }, onClick = { overflow = false; onRemove() })
                            DropdownMenuItem(text = { Text("Copy magnet link") }, onClick = { overflow = false; onCopyMagnet() })
                            Divider()
                            DropdownMenuItem(text = { Text("Speed") }, onClick = { overflow = false; onSpeed() })
                            DropdownMenuItem(text = { Text("DHT Info") }, onClick = { overflow = false; onDht() })
                            DropdownMenuItem(text = { Text("Logs") }, onClick = { overflow = false; onLogs() })
                            DropdownMenuItem(text = { Text("Settings") }, onClick = { overflow = false; onSettings() })
                        }
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ScrollableTabRow(selectedTabIndex = pager.currentPage, edgePadding = 8.dp) {
                TorrentDetailTab.entries.forEachIndexed { index, tab ->
                    Tab(
                        selected = pager.currentPage == index,
                        onClick = { scope.launch { pager.animateScrollToPage(index) } },
                        text = { Text(tab.label) },
                    )
                }
            }
            HorizontalPager(state = pager, modifier = Modifier.fillMaxSize()) { page ->
                DetailTabContent(
                    TorrentDetailTab.entries[page],
                    torrent,
                    state,
                    onSetFileWanted,
                    onDownloadFile,
                    onOpenFile,
                    onFilePage,
                    onTrackerPage,
                )
            }
        }
    }
}

@Composable
private fun DetailTabContent(
    tab: TorrentDetailTab,
    torrent: TorrentView?,
    state: ProductState,
    onSetFileWanted: (org.rstorrent.session.uniffi.FileView, Boolean) -> Unit,
    onDownloadFile: (org.rstorrent.session.uniffi.FileView) -> Unit,
    onOpenFile: (org.rstorrent.session.uniffi.FileView) -> Unit,
    onFilePage: (UInt) -> Unit,
    onTrackerPage: (UInt) -> Unit,
) {
    if (torrent == null) {
        CenterMessage("Torrent is no longer available")
        return
    }
    when (tab) {
        TorrentDetailTab.DETAILS ->
            DetailList(
                "Info hash" to torrent.torrentId,
                "State" to operationalLabel(torrent.operationalState),
                "Required" to formatBytes(torrent.requiredPayloadBytes),
                "Remaining" to formatBytes(torrent.remainingPayloadBytes),
                "Pieces" to "${torrent.verifiedPieceCount} / ${torrent.pieceCount}",
                "Trackers" to (torrent.configuredTrackerCount?.toString() ?: "—"),
            )
        TorrentDetailTab.STATUS ->
            DetailList(
                "Download" to formatRate(torrent.payloadDownloadRateBytes),
                "Peers" to torrent.activePeerConnections.toString(),
                "ETA" to torrentEta(torrent),
                "Storage" to torrent.storageState.name.lowercase(),
                "Progress" to torrent.progress.reason.name.lowercase(),
                "Error" to (torrent.error ?: "None"),
            )
        TorrentDetailTab.PIECES -> {
            val pieces = state.pieces[torrent.torrentId]
            if (pieces == null) {
                CenterMessage("Piece activity is loading…")
            } else {
                Column(Modifier.padding(16.dp)) {
                    Text("${pieces.verified.sumOf { (it.endExclusive - it.start).toInt() }} of ${pieces.pieceCount} verified")
                    Spacer(Modifier.height(12.dp))
                    PieceMap(pieces.pieceCount, pieces.verified, pieces.active)
                    DiskSummary(state.disk, torrent.torrentId)
                }
            }
        }
        TorrentDetailTab.FILES ->
            FilesScreen(
                state.files[torrent.torrentId],
                onSetFileWanted,
                onDownloadFile,
                onOpenFile,
                onFilePage,
            )
        TorrentDetailTab.TRACKERS ->
            TrackersScreen(state.trackers[torrent.torrentId], onTrackerPage)
        TorrentDetailTab.PEERS ->
            PeersScreen(
                state.peers[torrent.torrentId]?.values,
                state.swarms[torrent.torrentId],
            )
    }
}

@Composable
private fun DetailList(vararg rows: Pair<String, String>) {
    LazyColumn(contentPadding = PaddingValues(vertical = 8.dp)) {
        items(rows.toList()) { (label, value) -> ReadOnlySetting(label, value) }
    }
}

@Composable
private fun LogsShell(
    events: List<DiagnosticEvent>,
    sourceEvicted: String,
    localEvicted: ULong,
    resets: ULong,
    selectedTorrent: String?,
    service: ProductEngineService?,
    onBack: () -> Unit,
) {
    var profileName by rememberSaveable { mutableStateOf(DiagnosticProfile.NORMAL.name) }
    var severityName by rememberSaveable { mutableStateOf(DiagnosticSeverity.INFO.name) }
    var category by rememberSaveable { mutableStateOf("") }
    var torrentOnly by rememberSaveable { mutableStateOf(false) }
    var profileMenu by remember { mutableStateOf(false) }
    var severityMenu by remember { mutableStateOf(false) }
    var categoryMenu by remember { mutableStateOf(false) }
    LaunchedEffect(profileName, severityName, category, torrentOnly, selectedTorrent, service) {
        service?.configureDiagnostics(
            DiagnosticProfile.valueOf(profileName),
            DiagnosticSeverity.valueOf(severityName),
            category.takeIf(String::isNotEmpty)?.let { listOf(DiagnosticCategory(it)) }.orEmpty(),
            torrentOnly && selectedTorrent != null,
        )
    }
    SettingsPage("Logs", onBack) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Box {
                TextButton(onClick = { profileMenu = true }) {
                    Text("Profile: ${profileName.lowercase()}")
                }
                DropdownMenu(profileMenu, onDismissRequest = { profileMenu = false }) {
                    DiagnosticProfile.entries.forEach { profile ->
                        DropdownMenuItem(
                            text = { Text(profile.name.lowercase()) },
                            onClick = { profileName = profile.name; profileMenu = false },
                        )
                    }
                }
            }
            Box {
                TextButton(onClick = { severityMenu = true }) {
                    Text("Minimum: ${severityName.lowercase()}")
                }
                DropdownMenu(severityMenu, onDismissRequest = { severityMenu = false }) {
                    DiagnosticSeverity.entries.forEach { severity ->
                        DropdownMenuItem(
                            text = { Text(severity.name.lowercase()) },
                            onClick = { severityName = severity.name; severityMenu = false },
                        )
                    }
                }
            }
        }
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.weight(1f)) {
                TextButton(onClick = { categoryMenu = true }) {
                    Text("Category: ${category.ifEmpty { "all" }}")
                }
                DropdownMenu(categoryMenu, onDismissRequest = { categoryMenu = false }) {
                    LOG_CATEGORIES.forEach { option ->
                        DropdownMenuItem(
                            text = { Text(option.ifEmpty { "all" }) },
                            onClick = { category = option; categoryMenu = false },
                        )
                    }
                }
            }
            Text("Current torrent")
            Switch(
                checked = torrentOnly && selectedTorrent != null,
                onCheckedChange = { torrentOnly = it },
                enabled = selectedTorrent != null,
            )
        }
        Divider()
        ListItem(
            headlineContent = { Text("Delivery health") },
            supportingContent = {
                Text(
                    "source evicted $sourceEvicted · local evicted $localEvicted · " +
                        "subscription resets $resets",
                )
            },
        )
        Divider()
        if (events.isEmpty()) {
            Text(
                "No diagnostic records match the current filter",
                modifier = Modifier.padding(24.dp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
            events.takeLast(200).forEach { event ->
                ListItem(
                    headlineContent = { Text(event.code, fontFamily = FontFamily.Monospace) },
                    supportingContent = {
                        Column {
                            Text(
                                "${event.severity.name.lowercase()} · ${event.category.value}",
                                fontFamily = FontFamily.Monospace,
                            )
                            Text(event.message)
                        }
                    },
                )
                Divider()
            }
        }
    }
}

@Composable
private fun SettingsHub(navController: NavHostController) {
    SettingsPage("Settings", navController::popBackStack) {
        SettingsDestination("Storage", "Download folder and root health", Icons.Outlined.Folder) {
            navController.navigate(ProductRoutes.SETTINGS_STORAGE)
        }
        SettingsDestination("Speed & Connection Limits", "Peers, uploads, and active downloads", Icons.Outlined.Speed) {
            navController.navigate(ProductRoutes.SETTINGS_SPEED)
        }
        SettingsDestination("Notifications", "Android permission and channel", Icons.Outlined.Notifications) {
            navController.navigate(ProductRoutes.SETTINGS_NOTIFICATIONS)
        }
        SettingsDestination("Network & Privacy", "Listening, mapping, encryption, and IPv6", Icons.Outlined.NetworkCheck) {
            navController.navigate(ProductRoutes.SETTINGS_NETWORK)
        }
        SettingsDestination("Power Management", "Foreground operation and battery behavior", Icons.Outlined.BatterySaver) {
            navController.navigate(ProductRoutes.SETTINGS_POWER)
        }
        SettingsDestination("Advanced", "Appearance and unavailable expert features", Icons.Outlined.Settings) {
            navController.navigate(ProductRoutes.SETTINGS_ADVANCED)
        }
    }
}

@Composable
private fun SettingsDestination(
    title: String,
    detail: String,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    onClick: () -> Unit,
) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(detail) },
        leadingContent = { Icon(icon, contentDescription = null) },
        trailingContent = { Text("›") },
        modifier =
            Modifier.clickable(onClick = onClick)
                .semantics(mergeDescendants = true) { role = Role.Button },
    )
    Divider()
}

@Composable
private fun SimpleRouteScreen(
    title: String,
    onBack: () -> Unit,
    description: String,
) {
    SettingsPage(title, onBack) {
        Column(
            modifier = Modifier.fillMaxWidth().padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Icon(Icons.Outlined.Info, contentDescription = null, modifier = Modifier.size(48.dp))
            Spacer(Modifier.height(12.dp))
            Text(description, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun SettingsPage(
    title: String,
    onBack: () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()),
            content = content,
        )
    }
}

private val LOG_CATEGORIES =
    listOf(
        "",
        "lifecycle",
        "discovery",
        "tracker",
        "peer",
        "metadata",
        "scheduler",
        "piece",
        "storage",
        "integrity",
        "platform",
        "performance",
    )

@Composable
private fun ReadOnlySetting(
    title: String,
    detail: String,
) {
    ListItem(headlineContent = { Text(title) }, supportingContent = { Text(detail) })
    Divider()
}

@Composable
private fun UnavailableSetting(title: String) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text("Not available yet") },
        colors = androidx.compose.material3.ListItemDefaults.colors(
            headlineColor = MaterialTheme.colorScheme.onSurfaceVariant,
            supportingColor = MaterialTheme.colorScheme.outline,
        ),
    )
    Divider()
}

@Composable
private fun SettingAction(
    title: String,
    detail: String,
    onClick: () -> Unit,
    action: String,
) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(detail) },
        trailingContent = { TextButton(onClick = onClick) { Text(action) } },
    )
    Divider()
}

@Composable
private fun CenterMessage(message: String) {
    Box(Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
        Text(message, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun RemoveDialog(
    count: Int,
    onDismiss: () -> Unit,
    onKeep: () -> Unit,
    onDelete: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (count == 1) "Remove torrent?" else "Remove $count torrents?") },
        text = { Text("Choose whether RSTorrent should keep or delete its managed downloaded data.") },
        confirmButton = { TextButton(onClick = onDelete) { Text("Delete data") } },
        dismissButton = {
            Row {
                TextButton(onClick = onDismiss) { Text("Cancel") }
                TextButton(onClick = onKeep) { Text("Keep data") }
            }
        },
    )
}
