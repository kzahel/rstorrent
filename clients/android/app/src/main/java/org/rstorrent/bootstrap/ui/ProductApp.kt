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
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
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
import org.rstorrent.bootstrap.ProductNotificationNavigation
import org.rstorrent.bootstrap.ProductNotificationPreference
import org.rstorrent.bootstrap.ProductState
import org.rstorrent.bootstrap.ExternalIntakeKind
import org.rstorrent.bootstrap.ExternalIntakeNoticeKind
import org.rstorrent.bootstrap.ExternalIntakePhase
import org.rstorrent.bootstrap.ExternalIntakePresentation
import org.rstorrent.bootstrap.GlobalPresentation
import org.rstorrent.bootstrap.TorrentPresentation
import org.rstorrent.bootstrap.clientSettingsPatch
import org.rstorrent.bootstrap.presentedClientSettings
import org.rstorrent.bootstrap.presentedTorrent
import org.rstorrent.bootstrap.torrentSettingsPatch
import org.rstorrent.session.uniffi.DiagnosticEvent
import org.rstorrent.session.uniffi.DiagnosticCategory
import org.rstorrent.session.uniffi.DiagnosticProfile
import org.rstorrent.session.uniffi.DiagnosticSeverity
import org.rstorrent.session.uniffi.ClientSettingsPatch
import org.rstorrent.session.uniffi.FilePriority
import org.rstorrent.session.uniffi.RemovalDataPolicy
import org.rstorrent.session.uniffi.StorageRootAvailability
import org.rstorrent.session.uniffi.TorrentOperationalState
import org.rstorrent.session.uniffi.TorrentSettingsPatch
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
    stateOverride: ProductState? = null,
    notificationNavigation: ProductNotificationNavigation? = null,
    onNotificationNavigationConsumed: (Long) -> Unit = {},
    onUpdateNotificationPreference: ((ProductNotificationPreference, Boolean) -> Unit)? = null,
    onBackgroundDownloads: ((Boolean) -> Unit)? = null,
    onKeepSeedingInBackground: ((Boolean) -> Unit)? = null,
    onUnmeteredNetworksOnly: ((Boolean) -> Unit)? = null,
    onUpdateClientSettings: ((ClientSettingsPatch) -> Unit)? = null,
    onUpdateTorrentSettings: ((String, TorrentSettingsPatch) -> Unit)? = null,
    onRepairStorage: (String) -> Unit = {},
    onExternalStartContent: ((Long, Boolean) -> Unit)? = null,
    onConfirmExternalIntake: ((Long) -> Unit)? = null,
    onRetryExternalIntake: ((Long) -> Unit)? = null,
    onCancelExternalIntake: ((Long) -> Unit)? = null,
) {
    RstorrentTheme(mode = themeMode, dynamicColor = dynamicColor) {
        val state =
            if (stateOverride != null) {
                stateOverride
            } else if (service == null) {
                ProductState()
            } else {
                val collected by service.state.collectAsStateWithLifecycle()
                collected
            }
        val snackbar = remember { SnackbarHostState() }
        val notificationScope = rememberCoroutineScope()
        val lifecycleControlsEnabled =
            service != null ||
                onBackgroundDownloads != null ||
                onKeepSeedingInBackground != null
        LaunchedEffect(state.externalIntakeNotice?.sequence) {
            state.externalIntakeNotice?.let { notice ->
                snackbar.showSnackbar(externalIntakeNoticeText(notice.kind))
            }
        }
        Box(modifier = Modifier.fillMaxSize()) {
            Surface(modifier = Modifier.fillMaxSize()) {
                ProductNavHost(
                    state = state,
                    service = service,
                    onSelectStorage = onSelectStorage,
                    onRepairStorage = onRepairStorage,
                    onBrowseTorrent = onBrowseTorrent,
                    notificationsGranted = notificationsGranted,
                    onRequestNotifications = onRequestNotifications,
                    onOpenNotificationSettings = onOpenNotificationSettings,
                    notificationNavigation = notificationNavigation,
                    onNotificationNavigationConsumed = onNotificationNavigationConsumed,
                    onNotificationNavigationFallback = { message ->
                        notificationScope.launch { snackbar.showSnackbar(message) }
                    },
                    onUpdateNotificationPreference =
                        onUpdateNotificationPreference ?: { preference, enabled ->
                            service?.setNotificationPreference(preference, enabled)
                            Unit
                        },
                    onBackgroundDownloads =
                        onBackgroundDownloads ?: {
                            service?.setBackgroundDownloadsEnabled(it)
                            Unit
                        },
                    onKeepSeedingInBackground =
                        onKeepSeedingInBackground ?: {
                            service?.setKeepSeedingInBackground(it)
                            Unit
                        },
                    lifecycleControlsEnabled = lifecycleControlsEnabled,
                    onUnmeteredNetworksOnly =
                        onUnmeteredNetworksOnly ?: {
                            service?.setUnmeteredNetworksOnly(it)
                            Unit
                        },
                    themeMode = themeMode,
                    dynamicColor = dynamicColor,
                    onThemeMode = onThemeMode,
                    onDynamicColor = onDynamicColor,
                    onUpdateClientSettings =
                        onUpdateClientSettings ?: {
                            service?.updateClientSettings(it)
                            Unit
                        },
                    onUpdateTorrentSettings =
                        onUpdateTorrentSettings ?: { torrentId, patch ->
                            service?.updateTorrentSettings(torrentId, patch)
                            Unit
                        },
                )
            }
            SnackbarHost(
                hostState = snackbar,
                modifier = Modifier.align(Alignment.BottomCenter),
            )
        }
        state.externalIntake?.let { intake ->
            ExternalTorrentIntakeDialog(
                intake = intake,
                storageRootReady = state.storageRootReady,
                repairRootId = state.storage?.defaultRoot,
                onSelectStorage = onSelectStorage,
                onRepairStorage = onRepairStorage,
                onStartContent =
                    onExternalStartContent ?: { id, start ->
                        service?.setExternalIntakeStartContent(id, start)
                        Unit
                    },
                onConfirm =
                    onConfirmExternalIntake ?: {
                        service?.confirmExternalIntake(it)
                        Unit
                    },
                onRetry =
                    onRetryExternalIntake ?: {
                        service?.retryExternalIntake(it)
                        Unit
                    },
                onCancel =
                    onCancelExternalIntake ?: {
                        service?.cancelExternalIntake(it)
                        Unit
                    },
            )
        }
        state.companionPairing?.let { pairing ->
            AlertDialog(
                onDismissRequest = {},
                title = { Text("Connect Chrome extension?") },
                text = {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(pairing.extensionName)
                        Text(
                            "Extension ${pairing.extensionId}\n" +
                                "Installation ${pairing.installationId}",
                            style = MaterialTheme.typography.bodySmall,
                            fontFamily = FontFamily.Monospace,
                        )
                    }
                },
                confirmButton = {
                    TextButton(
                        onClick = { service?.approveCompanionPairing(pairing.requestId) },
                    ) {
                        Text("Approve")
                    }
                },
                dismissButton = {
                    TextButton(
                        onClick = { service?.rejectCompanionPairing(pairing.requestId) },
                    ) {
                        Text("Reject")
                    }
                },
            )
        }
    }
}

@Composable
private fun ExternalTorrentIntakeDialog(
    intake: ExternalIntakePresentation,
    storageRootReady: Boolean,
    repairRootId: String?,
    onSelectStorage: () -> Unit,
    onRepairStorage: (String) -> Unit,
    onStartContent: (Long, Boolean) -> Unit,
    onConfirm: (Long) -> Unit,
    onRetry: (Long) -> Unit,
    onCancel: (Long) -> Unit,
) {
    val title =
        when (intake.kind) {
            ExternalIntakeKind.MAGNET -> "Magnet link from another app"
            ExternalIntakeKind.TORRENT_FILE -> "Torrent file from another app"
        }
    AlertDialog(
        onDismissRequest = { onCancel(intake.intakeId) },
        title = { Text(title) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                intake.displayLabel?.let { Text(it) }
                Row(
                    modifier =
                        Modifier.fillMaxWidth().clickable(
                            enabled = intake.phase != ExternalIntakePhase.SUBMITTING,
                        ) {
                            onStartContent(intake.intakeId, !intake.startContent)
                        },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(
                        checked = intake.startContent,
                        onCheckedChange = {
                            onStartContent(intake.intakeId, it)
                        },
                        enabled = intake.phase != ExternalIntakePhase.SUBMITTING,
                    )
                    Text("Start downloading immediately")
                }
                if (!storageRootReady) {
                    Text("Choose or repair a download folder before adding this item.")
                    TextButton(
                        onClick = {
                            if (repairRootId == null) onSelectStorage()
                            else onRepairStorage(repairRootId)
                        },
                    ) {
                        Text(if (repairRootId == null) "Select folder" else "Repair folder")
                    }
                }
                when (intake.phase) {
                    ExternalIntakePhase.AWAITING_ROOT -> Unit
                    ExternalIntakePhase.SUBMITTING -> Text("Adding…")
                    ExternalIntakePhase.RETRYABLE_FAILURE ->
                        Text("The source could not be read. You can retry once.")
                    else -> Unit
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    if (intake.phase == ExternalIntakePhase.RETRYABLE_FAILURE) {
                        onRetry(intake.intakeId)
                    } else {
                        onConfirm(intake.intakeId)
                    }
                },
                enabled =
                    storageRootReady &&
                        intake.phase in
                        setOf(
                            ExternalIntakePhase.PRESENTED,
                            ExternalIntakePhase.RETRYABLE_FAILURE,
                        ),
            ) {
                Text(
                    if (intake.phase == ExternalIntakePhase.RETRYABLE_FAILURE) {
                        "Retry"
                    } else {
                        "Add"
                    },
                )
            }
        },
        dismissButton = {
            TextButton(onClick = { onCancel(intake.intakeId) }) {
                Text("Cancel")
            }
        },
    )
}

private fun externalIntakeNoticeText(kind: ExternalIntakeNoticeKind): String =
    when (kind) {
        ExternalIntakeNoticeKind.REJECTED -> "That item can’t be added."
        ExternalIntakeNoticeKind.QUEUE_FULL -> "Too many items are waiting to be added."
        ExternalIntakeNoticeKind.ADDED -> "Torrent added."
        ExternalIntakeNoticeKind.ALREADY_PRESENT -> "Torrent already present."
        ExternalIntakeNoticeKind.SELECTION_EXPANDED -> "Torrent selection updated."
        ExternalIntakeNoticeKind.TERMINAL_FAILURE -> "The torrent could not be added."
    }

@Composable
private fun ProductNavHost(
    state: ProductState,
    service: ProductEngineService?,
    onSelectStorage: () -> Unit,
    onRepairStorage: (String) -> Unit,
    onBrowseTorrent: (Boolean) -> Unit,
    notificationsGranted: Boolean,
    onRequestNotifications: () -> Unit,
    onOpenNotificationSettings: () -> Unit,
    notificationNavigation: ProductNotificationNavigation?,
    onNotificationNavigationConsumed: (Long) -> Unit,
    onNotificationNavigationFallback: (String) -> Unit,
    onUpdateNotificationPreference: (ProductNotificationPreference, Boolean) -> Unit,
    onBackgroundDownloads: (Boolean) -> Unit,
    onKeepSeedingInBackground: (Boolean) -> Unit,
    lifecycleControlsEnabled: Boolean,
    onUnmeteredNetworksOnly: (Boolean) -> Unit,
    themeMode: ProductThemeMode,
    dynamicColor: Boolean,
    onThemeMode: (ProductThemeMode) -> Unit,
    onDynamicColor: (Boolean) -> Unit,
    onUpdateClientSettings: (ClientSettingsPatch) -> Unit,
    onUpdateTorrentSettings: (String, TorrentSettingsPatch) -> Unit,
) {
    val navController = rememberNavController()
    var removeTargets by remember { mutableStateOf(emptySet<String>()) }
    var removeStorageRoot by remember { mutableStateOf<String?>(null) }
    var confirmKeepSeeding by remember { mutableStateOf(false) }
    LaunchedEffect(notificationNavigation?.sequence, state.ready) {
        val target = notificationNavigation ?: return@LaunchedEffect
        if (!state.ready) return@LaunchedEffect
        when (target) {
            is ProductNotificationNavigation.Torrent -> {
                if (target.torrentId in state.torrents) {
                    navController.navigate(ProductRoutes.detail(target.torrentId)) {
                        launchSingleTop = true
                    }
                } else {
                    if (!navController.popBackStack(ProductRoutes.LIBRARY, inclusive = false)) {
                        navController.navigate(ProductRoutes.LIBRARY) { launchSingleTop = true }
                    }
                    onNotificationNavigationFallback("That torrent is no longer available.")
                }
            }
            is ProductNotificationNavigation.StorageRepair -> {
                navController.navigate(ProductRoutes.SETTINGS_STORAGE) { launchSingleTop = true }
                if (
                    target.rootId != null &&
                    state.storage?.roots?.none { it.rootId == target.rootId } == true
                ) {
                    onNotificationNavigationFallback(
                        "That download folder is no longer registered.",
                    )
                }
            }
        }
        onNotificationNavigationConsumed(target.sequence)
    }
    LaunchedEffect(state.externalIntake?.intakeId) {
        if (state.externalIntake != null) {
            if (!navController.popBackStack(ProductRoutes.LIBRARY, inclusive = false)) {
                navController.navigate(ProductRoutes.LIBRARY) { launchSingleTop = true }
            }
        }
    }
    NavHost(navController = navController, startDestination = ProductRoutes.LIBRARY) {
        composable(ProductRoutes.LIBRARY) {
            LibraryScreen(
                state = state,
                notificationsGranted =
                    notificationsGranted &&
                        state.notifications.appNotificationsEnabled &&
                        state.notifications.backgroundChannelEnabled,
                onRequestNotifications =
                    if (notificationsGranted) {
                        onOpenNotificationSettings
                    } else {
                        onRequestNotifications
                    },
                notificationActionLabel = if (notificationsGranted) "Manage" else "Enable",
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
            val torrent = state.presentedTorrent(state.torrents[torrentId])
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
                onTransferLimits = { onUpdateTorrentSettings(torrentId, it) },
                onSpeed = { navController.navigate(ProductRoutes.SPEED) },
                onDht = { navController.navigate(ProductRoutes.DHT) },
                onLogs = { navController.navigate(ProductRoutes.LOGS) },
                onSettings = { navController.navigate(ProductRoutes.SETTINGS) },
                onPresent = { service?.presentTorrent(torrentId, it) },
                onSetFilePriority = { file, priority ->
                    service?.setFilePriority(torrentId, file.fileIndex, priority)
                },
                onDownloadFile = { service?.downloadFileNow(torrentId, it.fileIndex) },
                onOpenFile = { file ->
                    torrent?.displayName?.let {
                        service?.openCompletedFile(torrent.storageRoot, it, file)
                    }
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
            SpeedScreen(state.speed, state.currentRates, navController::popBackStack)
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
                    val isCurrent = root.rootId == state.storage?.defaultRoot
                    val isReferenced = state.torrents.values.any { it.storageRoot == root.rootId }
                    val title =
                        root.label +
                            if (isCurrent) " (current)" else ""
                    val detail =
                        root.availability.name.lowercase() +
                            (root.displayPath?.let { " · $it" } ?: " · Android document provider")
                    if (root.availability == StorageRootAvailability.UNAVAILABLE) {
                        SettingAction(
                            title = title,
                            detail = detail,
                            onClick = { onRepairStorage(root.rootId) },
                            action = "Repair",
                        )
                    } else if (!isCurrent) {
                        SettingAction(
                            title = title,
                            detail = detail,
                            onClick = { service?.makeSafRootCurrent(root.rootId) },
                            action = "Use",
                        )
                    } else {
                        ReadOnlySettingsRow(title, detail)
                    }
                    if (!isCurrent && !isReferenced) {
                        SettingAction(
                            title = "Forget ${root.label}",
                            detail = "Release Android access without deleting downloaded files.",
                            onClick = { removeStorageRoot = root.rootId },
                            action = "Remove",
                        )
                    }
                }
            }
        }
        composable(ProductRoutes.SETTINGS_SPEED) {
            SettingsPage("Speed & Connection Limits", navController::popBackStack) {
                val settings = state.presentedClientSettings()
                if (settings == null) {
                    Text("Settings are loading…", modifier = Modifier.padding(16.dp))
                } else {
                    ConnectionLimitsSettings(
                        settings,
                        onPeerConnections = { value ->
                            onUpdateClientSettings(
                                clientSettingsPatch(peerConnectionLimit = value),
                            )
                        },
                        onUploadSlots = { value ->
                            onUpdateClientSettings(clientSettingsPatch(uploadSlots = value))
                        },
                        onActiveDownloads = { value ->
                            onUpdateClientSettings(
                                clientSettingsPatch(activeDownloads = value),
                            )
                        },
                        onActiveSeeds = { value ->
                            onUpdateClientSettings(
                                clientSettingsPatch(activeSeeds = value),
                            )
                        },
                        onShareRatioLimit = { value ->
                            onUpdateClientSettings(
                                clientSettingsPatch(shareRatioLimitPercent = value),
                            )
                        },
                        onFinishedDownloadRatioLimit = { value ->
                            onUpdateClientSettings(
                                clientSettingsPatch(
                                    finishedDownloadRatioLimitPercent = value,
                                ),
                            )
                        },
                        onFinishedTimeLimit = { value ->
                            onUpdateClientSettings(
                                clientSettingsPatch(finishedTimeLimitSeconds = value),
                            )
                        },
                        onUploadRateLimit = { value ->
                            onUpdateClientSettings(
                                clientSettingsPatch(uploadRateLimit = value),
                            )
                        },
                        onDownloadRateLimit = { value ->
                            onUpdateClientSettings(
                                clientSettingsPatch(downloadRateLimit = value),
                            )
                        },
                    )
                }
            }
        }
        composable(ProductRoutes.SETTINGS_NOTIFICATIONS) {
            SettingsPage("Notifications", navController::popBackStack) {
                val notificationState = state.notifications
                val backgroundVisible =
                    notificationsGranted &&
                        notificationState.appNotificationsEnabled &&
                        notificationState.backgroundChannelEnabled
                SettingAction(
                    title =
                        when {
                            !notificationsGranted -> "Notifications disabled"
                            !notificationState.appNotificationsEnabled -> "Notifications blocked"
                            !notificationState.backgroundChannelEnabled ->
                                "Background activity blocked"
                            else -> "Notifications enabled"
                        },
                    detail =
                        if (backgroundVisible) {
                            "Android can show foreground status. Background lifetime is still provisional."
                        } else {
                            "RSTorrent works while Android is visible. Leaving Android stops background work."
                        },
                    onClick = if (notificationsGranted) onOpenNotificationSettings else onRequestNotifications,
                    action = if (notificationsGranted) "Manage" else "Enable",
                )
                NotificationToggleSetting(
                    title = "Download completed",
                    detail =
                        if (notificationState.completionChannelEnabled) {
                            "Notify when a download genuinely finishes."
                        } else {
                            "Blocked in Android system settings."
                        },
                    checked = notificationState.preferences.downloadComplete,
                    onChecked = {
                        onUpdateNotificationPreference(
                            ProductNotificationPreference.DOWNLOAD_COMPLETE,
                            it,
                        )
                    },
                )
                NotificationToggleSetting(
                    title = "Needs attention",
                    detail =
                        if (notificationState.attentionChannelEnabled) {
                            "Notify when a torrent or download folder needs repair."
                        } else {
                            "Blocked in Android system settings."
                        },
                    checked = notificationState.preferences.needsAttention,
                    onChecked = {
                        onUpdateNotificationPreference(
                            ProductNotificationPreference.NEEDS_ATTENTION,
                            it,
                        )
                    },
                )
                notificationState.preferenceError?.let {
                    ReadOnlySetting("Setting not saved", it)
                }
                SettingAction(
                    title = "Manage system notification settings",
                    detail = "Review Android app and channel controls.",
                    onClick = onOpenNotificationSettings,
                    action = "Open",
                )
            }
        }
        composable(ProductRoutes.SETTINGS_NETWORK) {
            SettingsPage("Network & Privacy", navController::popBackStack) {
                state.presentedClientSettings()?.let { settings ->
                    NetworkSettings(
                        settings,
                        network = state.network,
                        onUnmeteredNetworksOnly = onUnmeteredNetworksOnly,
                        onListener = { enabled ->
                            onUpdateClientSettings(
                                clientSettingsPatch(
                                    listener =
                                        if (enabled) {
                                            org.rstorrent.session.uniffi.ListenerPolicy.AutomaticLocalNetwork
                                        } else {
                                            org.rstorrent.session.uniffi.ListenerPolicy.Disabled
                                        },
                                ),
                            )
                        },
                        onPortMapping = { enabled ->
                            onUpdateClientSettings(
                                clientSettingsPatch(
                                    portMapping =
                                        if (enabled) {
                                            org.rstorrent.session.uniffi.PortMappingPolicy.UPNP
                                        } else {
                                            org.rstorrent.session.uniffi.PortMappingPolicy.DISABLED
                                        },
                                ),
                            )
                        },
                        onIpv6 = { enabled ->
                            onUpdateClientSettings(clientSettingsPatch(ipv6Enabled = enabled))
                        },
                        onEncryption = { policy ->
                            onUpdateClientSettings(clientSettingsPatch(encryption = policy))
                        },
                    )
                } ?: Text("Settings are loading…", modifier = Modifier.padding(16.dp))
            }
        }
        composable(ProductRoutes.SETTINGS_POWER) {
            SettingsPage("Power Management", navController::popBackStack) {
                val lifecycle = state.lifecycle
                ListItem(
                    headlineContent = { Text("Continue downloads in background") },
                    supportingContent = {
                        Text(
                            when {
                                lifecycle.effectiveBackgroundDownloads ->
                                    "Uses a visible notification and stops when selected work completes."
                                lifecycle.backgroundDownloadsEnabled ->
                                    "Configured, but Android notification settings currently block it."
                                else ->
                                    "Allow selected downloads and checks to continue after leaving RSTorrent."
                            },
                        )
                    },
                    trailingContent = {
                        Switch(
                            checked = lifecycle.backgroundDownloadsEnabled,
                            onCheckedChange = onBackgroundDownloads,
                            enabled = lifecycleControlsEnabled,
                            modifier =
                                Modifier.semantics {
                                    contentDescription = "Continue downloads in background"
                                },
                        )
                    },
                )
                ListItem(
                    headlineContent = { Text("Keep seeding in background") },
                    supportingContent = {
                        Text(
                            if (lifecycle.backgroundDownloadsEnabled) {
                                "Keep completed, desired-running torrents active after downloads finish."
                            } else {
                                "Enable background downloads first."
                            },
                        )
                    },
                    trailingContent = {
                        Switch(
                            checked = lifecycle.keepSeedingEnabled,
                            onCheckedChange = { enabled ->
                                if (enabled) confirmKeepSeeding = true
                                else onKeepSeedingInBackground(false)
                            },
                            enabled =
                                lifecycleControlsEnabled &&
                                    lifecycle.backgroundDownloadsEnabled,
                            modifier =
                                Modifier.semantics {
                                    contentDescription = "Keep seeding in background"
                                },
                        )
                    },
                )
                ListItem(
                    headlineContent = {
                        Text("Prevent sleep during active downloads and checks")
                    },
                    supportingContent = {
                        Text(
                            "Keeps the CPU active while starting, downloading, or checking. " +
                                "The display may turn off normally.",
                        )
                    },
                    trailingContent = {
                        Switch(
                            checked = state.preventSleepDuringActiveDownloads,
                            onCheckedChange = {
                                service?.setPreventSleepDuringActiveDownloads(it)
                            },
                            enabled = service != null,
                        )
                    },
                )
                ReadOnlySetting(
                    "Android background limits",
                    "Android may limit a long background session. Open RSTorrent again to continue; force-stop and reboot do not restart it.",
                )
                lifecycle.preferenceError?.let {
                    ReadOnlySetting("Setting not saved", it)
                }
                UnavailableSetting("Low-battery shutdown")
            }
            if (confirmKeepSeeding) {
                AlertDialog(
                    onDismissRequest = { confirmKeepSeeding = false },
                    title = { Text("Keep seeding in background?") },
                    text = {
                        Text(
                            "Seeding can use battery and data after downloads finish. " +
                                "Android may still stop a long session.",
                        )
                    },
                    confirmButton = {
                        TextButton(
                            onClick = {
                                confirmKeepSeeding = false
                                onKeepSeedingInBackground(true)
                            },
                        ) {
                            Text("Keep seeding")
                        }
                    },
                    dismissButton = {
                        TextButton(onClick = { confirmKeepSeeding = false }) {
                            Text("Cancel")
                        }
                    },
                )
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
                HorizontalDivider()
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
                removeTargets.forEach { service?.removeTorrent(it, RemovalDataPolicy.DELETE_DATA) }
                removeTargets = emptySet()
            },
        )
    }
    removeStorageRoot?.let { rootId ->
        val label = state.storage?.roots?.singleOrNull { it.rootId == rootId }?.label ?: rootId
        AlertDialog(
            onDismissRequest = { removeStorageRoot = null },
            title = { Text("Forget $label?") },
            text = {
                Text(
                    "RSTorrent will release access to this folder. Existing files are not deleted.",
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        service?.removeSafRoot(rootId)
                        removeStorageRoot = null
                    },
                ) {
                    Text("Remove")
                }
            },
            dismissButton = {
                TextButton(onClick = { removeStorageRoot = null }) {
                    Text("Cancel")
                }
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
    onTransferLimits: (TorrentSettingsPatch) -> Unit,
    onSpeed: () -> Unit,
    onDht: () -> Unit,
    onLogs: () -> Unit,
    onSettings: () -> Unit,
    onPresent: (TorrentPresentation) -> Unit,
    onSetFilePriority: (org.rstorrent.session.uniffi.FileView, FilePriority) -> Unit,
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
                        torrent?.let(::torrentPresentationName) ?: "Torrent",
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
                            HorizontalDivider()
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
                    onSetFilePriority,
                    onDownloadFile,
                    onOpenFile,
                    onFilePage,
                    onTrackerPage,
                    onTransferLimits,
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
    onSetFilePriority: (org.rstorrent.session.uniffi.FileView, FilePriority) -> Unit,
    onDownloadFile: (org.rstorrent.session.uniffi.FileView) -> Unit,
    onOpenFile: (org.rstorrent.session.uniffi.FileView) -> Unit,
    onFilePage: (UInt) -> Unit,
    onTrackerPage: (UInt) -> Unit,
    onTransferLimits: (TorrentSettingsPatch) -> Unit,
) {
    if (torrent == null) {
        CenterMessage("Torrent is no longer available")
        return
    }
    when (tab) {
        TorrentDetailTab.DETAILS ->
            TorrentDetails(
                torrent,
                onTransferLimits,
                *buildList {
                    val v1 = torrent.protocolIdentities.v1
                    val v2 = torrent.protocolIdentities.v2
                    if (v1 != null && v2 != null) {
                        add("Info hash (v1)" to v1)
                        add("Info hash (v2)" to v2)
                    } else {
                        add("Info hash" to (v1 ?: v2 ?: "—"))
                    }
                    add("State" to operationalLabel(torrent.operationalState))
                    add("Required" to formatBytes(torrent.requiredPayloadBytes))
                    add("Remaining" to formatBytes(torrent.remainingPayloadBytes))
                    add("Pieces" to "${torrent.verifiedPieceCount} / ${torrent.pieceCount}")
                    add("Trackers" to (torrent.configuredTrackerCount?.toString() ?: "—"))
                    add("Lifetime downloaded" to formatBytes(torrent.lifetime.downloadedPayloadBytes))
                    add("Lifetime uploaded" to formatBytes(torrent.lifetime.uploadedPayloadBytes))
                    add("Share ratio" to formatShareRatio(torrent.lifetime.shareRatioHundredths))
                    add("Active time" to formatDuration(torrent.lifetime.activeSeconds))
                    add("Finished time" to formatDuration(torrent.lifetime.finishedSeconds))
                    add("Seeding time" to formatDuration(torrent.lifetime.seedingSeconds))
                    if (torrent.seeding.goal != null) {
                        add("Seed admission" to seedAdmissionLabel(torrent.seeding.admission))
                        add("Seeding priority goal" to seedGoalLabel(torrent.seeding.goal))
                        add(
                            "Goal behavior" to
                                "Goals affect priority; a goal-met torrent may continue seeding",
                        )
                    }
                }.toTypedArray(),
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
                onSetFilePriority,
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
private fun TorrentDetails(
    torrent: TorrentView,
    onTransferLimits: (TorrentSettingsPatch) -> Unit,
    vararg rows: Pair<String, String>,
) {
    LazyColumn(contentPadding = PaddingValues(vertical = 8.dp)) {
        items(rows.toList()) { (label, value) -> ReadOnlySetting(label, value) }
        item("download-rate-limit") {
            RateLimitSetting(
                title = "Torrent download limit",
                configured = torrent.transferLimits.download,
                onValue = { limit ->
                    onTransferLimits(torrentSettingsPatch(downloadRateLimit = limit))
                },
            )
        }
        item("upload-rate-limit") {
            RateLimitSetting(
                title = "Torrent upload limit",
                configured = torrent.transferLimits.upload,
                onValue = { limit ->
                    onTransferLimits(torrentSettingsPatch(uploadRateLimit = limit))
                },
            )
        }
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
        HorizontalDivider()
        ListItem(
            headlineContent = { Text("Delivery health") },
            supportingContent = {
                Text(
                    "source evicted $sourceEvicted · local evicted $localEvicted · " +
                        "subscription resets $resets",
                )
            },
        )
        HorizontalDivider()
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
                HorizontalDivider()
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
    HorizontalDivider()
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
    HorizontalDivider()
}

@Composable
private fun NotificationToggleSetting(
    title: String,
    detail: String,
    checked: Boolean,
    onChecked: (Boolean) -> Unit,
) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(detail) },
        trailingContent = { Switch(checked = checked, onCheckedChange = null) },
        modifier =
            Modifier.clickable { onChecked(!checked) }
                .semantics(mergeDescendants = true) { role = Role.Switch },
    )
    HorizontalDivider()
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
    HorizontalDivider()
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
    HorizontalDivider()
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
