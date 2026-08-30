package org.rstorrent.bootstrap

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.rstorrent.bootstrap.ui.ProductApp
import org.rstorrent.bootstrap.ui.ProductThemeMode
import org.rstorrent.session.uniffi.ProgressAssessment
import org.rstorrent.session.uniffi.ProgressDisposition
import org.rstorrent.session.uniffi.ProgressPhase
import org.rstorrent.session.uniffi.ProgressReason
import org.rstorrent.session.uniffi.AdvertisedPeerEndpointStatus
import org.rstorrent.session.uniffi.BandwidthDirectionRuntimeView
import org.rstorrent.session.uniffi.BandwidthRuntimeView
import org.rstorrent.session.uniffi.ClientSettings
import org.rstorrent.session.uniffi.ClientSettingsApplicationState
import org.rstorrent.session.uniffi.ClientSettingsPatch
import org.rstorrent.session.uniffi.ClientSettingsRuntimeView
import org.rstorrent.session.uniffi.EffectiveListenerSettings
import org.rstorrent.session.uniffi.EncryptionPolicy
import org.rstorrent.session.uniffi.HttpsServerAuthenticationPolicy
import org.rstorrent.session.uniffi.Ipv6PinholeStatus
import org.rstorrent.session.uniffi.ListenerPolicy
import org.rstorrent.session.uniffi.ListenerStatus
import org.rstorrent.session.uniffi.PortMappingPolicy
import org.rstorrent.session.uniffi.PortMappingStatus
import org.rstorrent.session.uniffi.SessionUdpStatus
import org.rstorrent.session.uniffi.StorageState
import org.rstorrent.session.uniffi.TorrentTransferLimits
import org.rstorrent.session.uniffi.TransferRateLimit
import org.rstorrent.session.uniffi.TorrentEtaView
import org.rstorrent.session.uniffi.TorrentOperationalState
import org.rstorrent.session.uniffi.TorrentProtocolIdentities
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentView
import org.rstorrent.session.uniffi.TorrentSettingsPatch

class ProductNavigationTest {
    @get:Rule val compose = createComposeRule()

    @Test
    fun libraryReachesSettingsHierarchyAndAddIntake() {
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
            )
        }

        compose.onNodeWithContentDescription("Add torrent").performClick()
        compose.onNodeWithText("Browse .torrent file").assertIsDisplayed()
        compose.onNodeWithText("Cancel").performClick()

        compose.onNodeWithContentDescription("More options").performClick()
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText("Storage").performClick()
        compose.onNodeWithText("Download folder").assertIsDisplayed()
    }

    @Test
    fun liveTorrentReachesAllDetailAndGlobalRoutes() {
        val torrent = torrent()
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.DARK,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
                stateOverride =
                    ProductState(
                        ready = true,
                        storageRootReady = true,
                        torrents = mapOf(torrent.torrentId to torrent),
                    ),
            )
        }

        compose.onNodeWithText("Fixture torrent").performClick()
        compose.onNodeWithText("Info hash (v1)").assertIsDisplayed()
        compose.onNodeWithText("Info hash (v2)").assertIsDisplayed()
        listOf("Details", "Status", "Files", "Trackers", "Peers", "Pieces").forEach {
            compose.onNodeWithText(it).performScrollTo().assertIsDisplayed().performClick()
        }
        compose.onNodeWithContentDescription("Back").performClick()

        listOf("Speed", "DHT Info", "Logs").forEach { route ->
            compose.onNodeWithContentDescription("More options").performClick()
            compose.onNodeWithText(route).performClick()
            compose.onNodeWithText(route).assertIsDisplayed()
            compose.onNodeWithContentDescription("Back").performClick()
        }
    }

    @Test
    fun torrentRateEditorSurvivesFreshActiveRowsAndEmitsOneDirection() {
        val initial =
            torrent().copy(
                transferLimits =
                    TorrentTransferLimits(
                        TransferRateLimit.Limited(128U * 1_024U),
                        TransferRateLimit.Limited(256U * 1_024U),
                    ),
            )
        var state by
            mutableStateOf(
                ProductState(
                    ready = true,
                    storageRootReady = true,
                    torrents = mapOf(initial.torrentId to initial),
                ),
            )
        val patches = mutableListOf<TorrentSettingsPatch>()
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
                stateOverride = state,
                onUpdateTorrentSettings = { _, patch -> patches += patch },
            )
        }

        compose.onNodeWithText("Fixture torrent").performClick()
        compose.onNodeWithText("Torrent download limit").performScrollTo().performClick()
        compose.onNode(isToggleable()).performClick().assertIsOn()
        repeat(24) { index ->
            compose.runOnIdle {
                val fresh =
                    initial.copy(
                        transferLimits =
                            TorrentTransferLimits(
                                TransferRateLimit.Limited(128U * 1_024U),
                                TransferRateLimit.Limited(256U * 1_024U),
                            ),
                        receivedBytes = (8_192 + index).toString(),
                    )
                state = state.copy(torrents = mapOf(fresh.torrentId to fresh))
            }
        }
        compose.onNode(isToggleable()).assertIsOn()
        compose.onNodeWithText("Apply").performClick()

        assertEquals(1, patches.size)
        assertNull(patches.single().uploadRateLimit)
        assertEquals(TransferRateLimit.Unlimited, patches.single().downloadRateLimit)
    }

    @Test
    fun globalRateEditorEmitsOneTypedProperty() {
        val patches = mutableListOf<ClientSettingsPatch>()
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
                stateOverride =
                    ProductState(
                        ready = true,
                        storageRootReady = true,
                        clientSettings = clientSettings(),
                    ),
                onUpdateClientSettings = { patches += it },
            )
        }

        compose.onNodeWithContentDescription("More options").performClick()
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText("Speed & Connection Limits").performClick()
        compose.onNodeWithText("All torrents download limit").performClick()
        compose.onNode(isToggleable()).performClick().assertIsOn()
        compose.onNodeWithText("Apply").performClick()

        assertEquals(1, patches.size)
        assertEquals(TransferRateLimit.Unlimited, patches.single().downloadRateLimit)
        assertNull(patches.single().uploadRateLimit)
        assertNull(patches.single().peerConnectionLimit)
    }

    @Test
    fun externalMagnetUsesGenericConfirmationAndTypedCallbacks() {
        val startChoices = mutableListOf<Pair<Long, Boolean>>()
        val confirmations = mutableListOf<Long>()
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
                stateOverride =
                    ProductState(
                        ready = true,
                        storageRootReady = true,
                        externalIntake =
                            ExternalIntakePresentation(
                                41,
                                ExternalIntakeKind.MAGNET,
                                ExternalIntakePhase.PRESENTED,
                                null,
                                true,
                            ),
                    ),
                onExternalStartContent = { id, start -> startChoices += id to start },
                onConfirmExternalIntake = { confirmations += it },
            )
        }

        compose.onNodeWithText("Magnet link from another app").assertIsDisplayed()
        compose.onNodeWithText("Start downloading immediately").performClick()
        compose.onNodeWithText("Add").performClick()
        compose.onAllNodesWithText("secret.invalid", substring = true).assertCountEquals(0)
        assertEquals(listOf(41L to false), startChoices)
        assertEquals(listOf(41L), confirmations)
    }

    @Test
    fun externalTorrentWaitsForRootAndOffersCancel() {
        val selections = mutableListOf<Unit>()
        val cancellations = mutableListOf<Long>()
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = { selections += Unit },
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
                stateOverride =
                    ProductState(
                        ready = true,
                        externalIntake =
                            ExternalIntakePresentation(
                                42,
                                ExternalIntakeKind.TORRENT_FILE,
                                ExternalIntakePhase.AWAITING_ROOT,
                                "safe.torrent",
                                true,
                            ),
                    ),
                onCancelExternalIntake = { cancellations += it },
            )
        }

        compose.onNodeWithText("Torrent file from another app").assertIsDisplayed()
        compose.onNodeWithText("safe.torrent").assertIsDisplayed()
        compose.onNodeWithText("Add").assertIsNotEnabled()
        compose.onNode(
            hasText("Select folder") and hasAnyAncestor(isDialog()),
        ).performClick()
        compose.onNodeWithText("Cancel").performClick()
        assertEquals(1, selections.size)
        assertEquals(listOf(42L), cancellations)
    }

    @Test
    fun externalRetryIsExplicitAndBoundedByServiceState() {
        val retries = mutableListOf<Long>()
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
                stateOverride =
                    ProductState(
                        ready = true,
                        storageRootReady = true,
                        externalIntake =
                            ExternalIntakePresentation(
                                43,
                                ExternalIntakeKind.TORRENT_FILE,
                                ExternalIntakePhase.RETRYABLE_FAILURE,
                                null,
                                true,
                            ),
                    ),
                onRetryExternalIntake = { retries += it },
            )
        }

        compose.onNodeWithText("The source could not be read. You can retry once.")
            .assertIsDisplayed()
        compose.onNodeWithText("Retry").performClick()
        assertEquals(listOf(43L), retries)
    }

    @Test
    fun notificationSettingsExposePermissionChannelsAndDurableChoices() {
        val changes = mutableListOf<Pair<ProductNotificationPreference, Boolean>>()
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
                stateOverride =
                    ProductState(
                        ready = true,
                        notifications =
                            ProductNotificationState(
                                permissionGranted = true,
                                completionChannelEnabled = false,
                            ),
                    ),
                onUpdateNotificationPreference = { preference, enabled ->
                    changes += preference to enabled
                },
            )
        }

        compose.onNodeWithContentDescription("More options").performClick()
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText("Notifications").performClick()
        compose.onNodeWithText("Notifications enabled").assertIsDisplayed()
        compose.onNodeWithText("Blocked in Android system settings.").assertIsDisplayed()
        compose.onNodeWithText("Download completed").performClick()
        compose.onNodeWithText("Needs attention").performClick()
        compose.onNodeWithText("Manage system notification settings").assertIsDisplayed()
        assertEquals(
            listOf(
                ProductNotificationPreference.DOWNLOAD_COMPLETE to false,
                ProductNotificationPreference.NEEDS_ATTENTION to false,
            ),
            changes,
        )
    }

    @Test
    fun blockedBackgroundChannelExplainsVisibleOnlyOperation() {
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
                stateOverride =
                    ProductState(
                        ready = true,
                        notifications =
                            ProductNotificationState(
                                permissionGranted = true,
                                backgroundChannelEnabled = false,
                            ),
                    ),
            )
        }

        compose.onNodeWithContentDescription("More options").performClick()
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText("Notifications").performClick()
        compose.onNodeWithText("Background activity blocked").assertIsDisplayed()
        compose.onNodeWithText(
            "RSTorrent works while Android is visible. Leaving Android stops background work.",
        ).assertIsDisplayed()
    }

    @Test
    fun notificationNavigationSelectsExactTorrentAndConsumesOnce() {
        val consumed = mutableListOf<Long>()
        val torrent = torrent()
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
                stateOverride =
                    ProductState(
                        ready = true,
                        torrents = mapOf(torrent.torrentId to torrent),
                    ),
                notificationNavigation =
                    ProductNotificationNavigation.Torrent(7L, torrent.torrentId),
                onNotificationNavigationConsumed = { consumed += it },
            )
        }

        compose.onNodeWithText("Info hash (v1)").assertIsDisplayed()
        assertEquals(listOf(7L), consumed)
    }

    @Test
    fun staleNotificationTargetFallsBackToLibrary() {
        val consumed = mutableListOf<Long>()
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
                stateOverride = ProductState(ready = true),
                notificationNavigation =
                    ProductNotificationNavigation.Torrent(
                        8L,
                        "t1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    ),
                onNotificationNavigationConsumed = { consumed += it },
            )
        }

        compose.onNodeWithText("That torrent is no longer available.").assertIsDisplayed()
        assertEquals(listOf(8L), consumed)
    }

    private fun torrent(): TorrentView =
        TorrentView(
            torrentId = "t1-0123456789abcdef0123456789abcdef",
            protocolIdentities =
                TorrentProtocolIdentities(
                    v1 = "0123456789abcdef0123456789abcdef01234567",
                    v2 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                ),
            displayName = "Fixture torrent",
            sourceDisplayName = null,
            state = TorrentState.DOWNLOADING,
            operationalState = TorrentOperationalState.DOWNLOADING,
            downloadQueuePosition = 1U,
            transferLimits =
                TorrentTransferLimits(
                    TransferRateLimit.Unlimited,
                    TransferRateLimit.Unlimited,
                ),
            storageState = StorageState.AVAILABLE,
            storageRoot = "downloads",
            metadataAvailable = true,
            pieceCount = 4U,
            totalSizeBytes = "65536",
            verifiedPieceCount = 1U,
            requestedBytes = "16384",
            receivedBytes = "8192",
            storedBytes = "4096",
            activePeerConnections = 1U,
            configuredTrackerCount = 1U,
            payloadDownloadRateBytes = "4096",
            requiredPayloadBytes = "65536",
            remainingPayloadBytes = "49152",
            etaPayloadDownloadRateBytes = "4096",
            eta = TorrentEtaView.Estimate("12"),
            progress =
                ProgressAssessment(
                    ProgressDisposition.ACTIVE,
                    ProgressPhase.TRANSFER,
                    ProgressReason.TRANSFERRING_PIECES,
                    emptyList(),
                ),
            checking = null,
            archived = false,
            removalState = null,
            deleteDataSupported = true,
            forceRecheckAvailable = true,
            error = null,
        )

    private fun clientSettings(): ClientSettingsRuntimeView {
        val configured =
            ClientSettings(
                listener = ListenerPolicy.Disabled,
                preferredListenPort = 6_881U.toUShort(),
                portMapping = PortMappingPolicy.DISABLED,
                peerConnectionLimit = 200U,
                uploadSlots = 8U.toUShort(),
                activeDownloads = 3U.toUShort(),
                uploadRateLimit = TransferRateLimit.Limited(128U * 1_024U),
                downloadRateLimit = TransferRateLimit.Limited(256U * 1_024U),
                encryption = EncryptionPolicy.ALLOW,
                ipv6Enabled = true,
                trackerHttpsServerAuthentication =
                    HttpsServerAuthenticationPolicy.SYSTEM_TRUST,
            )
        return ClientSettingsRuntimeView(
            configured = configured,
            effectiveListener =
                EffectiveListenerSettings(
                    listener = ListenerPolicy.Disabled,
                    preferredListenPort = 6_881U.toUShort(),
                ),
            effectivePortMapping = PortMappingPolicy.DISABLED,
            effectivePeerConnectionLimit = 200U,
            effectiveUploadSlots = 8U.toUShort(),
            effectiveActiveDownloads = 3U.toUShort(),
            effectiveUploadRateLimit = configured.uploadRateLimit,
            effectiveDownloadRateLimit = configured.downloadRateLimit,
            activeDownloadsClampReason = null,
            activeDownloadCount = 0U.toUShort(),
            checkingCount = 0U.toUShort(),
            effectiveEncryption = EncryptionPolicy.ALLOW,
            effectiveIpv6Enabled = true,
            effectiveTrackerHttpsServerAuthentication =
                HttpsServerAuthenticationPolicy.SYSTEM_TRUST,
            transportApplication = ClientSettingsApplicationState.Applied,
            portMappingApplication = ClientSettingsApplicationState.Applied,
            peerConnectionsApplication = ClientSettingsApplicationState.Applied,
            uploadSlotsApplication = ClientSettingsApplicationState.Applied,
            bandwidthApplication = ClientSettingsApplicationState.Applied,
            bandwidth = BandwidthRuntimeView(bandwidthDirection(), bandwidthDirection()),
            encryptionApplication = ClientSettingsApplicationState.Applied,
            ipv6Application = ClientSettingsApplicationState.Applied,
            trackerHttpsAuthenticationApplication = ClientSettingsApplicationState.Applied,
            listenerStatus = ListenerStatus.Disabled,
            sessionUdpStatus = SessionUdpStatus.Unavailable,
            portMappingStatus = PortMappingStatus.Disabled,
            udpPortMappingStatus = PortMappingStatus.Disabled,
            ipv6PinholeStatus = Ipv6PinholeStatus.Disabled,
            advertisedPeerEndpoint = AdvertisedPeerEndpointStatus.Unavailable,
            transportFamilies = emptyList(),
        )
    }

    private fun bandwidthDirection(): BandwidthDirectionRuntimeView =
        BandwidthDirectionRuntimeView(
            registeredTorrents = 0U,
            activeWaiters = 0U,
            queuedRequestedBytes = "0",
            grantedBytes = "0",
            returnedBytes = "0",
            cancelledRequests = "0",
            throttleWaitMicros = "0",
            throttleWaitHighWaterMicros = "0",
            currentBurstCreditBytes = "0",
        )
}
