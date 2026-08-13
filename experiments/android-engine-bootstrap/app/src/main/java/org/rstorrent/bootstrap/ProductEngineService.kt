package org.rstorrent.bootstrap

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Binder
import android.os.Build
import android.os.CancellationSignal
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.rstorrent.bootstrap.uniffi.AndroidApplicationClient
import org.rstorrent.bootstrap.uniffi.AndroidApplicationConfig
import org.rstorrent.bootstrap.uniffi.AndroidNetworkPolicy
import org.rstorrent.bootstrap.uniffi.AndroidViewSubscription
import org.rstorrent.bootstrap.uniffi.SafStorageFailureKind
import org.rstorrent.bootstrap.uniffi.SafStorageOperation
import org.rstorrent.session.uniffi.Command
import org.rstorrent.session.uniffi.CommandResult
import org.rstorrent.session.uniffi.AddTorrentBytesRequest
import org.rstorrent.session.uniffi.CatalogPageRequest
import org.rstorrent.session.uniffi.ClientSettings
import org.rstorrent.session.uniffi.ClientSettingsApplicationState
import org.rstorrent.session.uniffi.ClientSettingsRuntimeView
import org.rstorrent.session.uniffi.DeliveryPolicy
import org.rstorrent.session.uniffi.DiagnosticCategory
import org.rstorrent.session.uniffi.DiagnosticFilter
import org.rstorrent.session.uniffi.DiagnosticProfile
import org.rstorrent.session.uniffi.DiagnosticSeverity
import org.rstorrent.session.uniffi.DiagnosticValue
import org.rstorrent.session.uniffi.EncryptionPolicy
import org.rstorrent.session.uniffi.FileSelectionIntent
import org.rstorrent.session.uniffi.FilePriority
import org.rstorrent.session.uniffi.FileView
import org.rstorrent.session.uniffi.HttpsServerAuthenticationPolicy
import org.rstorrent.session.uniffi.ListenerPolicy
import org.rstorrent.session.uniffi.ListenerStatus
import org.rstorrent.session.uniffi.PortMappingPolicy
import org.rstorrent.session.uniffi.RequestEnvelope
import org.rstorrent.session.uniffi.RemovalState
import org.rstorrent.session.uniffi.RemovalDataPolicy
import org.rstorrent.session.uniffi.ResponseOutcome
import org.rstorrent.session.uniffi.SpeedMetric
import org.rstorrent.session.uniffi.SpeedRange
import org.rstorrent.session.uniffi.SubscriptionSpec
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentTransferLimits
import org.rstorrent.session.uniffi.TransferRateLimit
import org.rstorrent.session.uniffi.ViewProjection
import org.rstorrent.session.uniffi.ViewSelector
import org.rstorrent.session.uniffi.ViewPatch
import org.rstorrent.session.uniffi.ViewSnapshot
import org.rstorrent.session.uniffi.ViewUpdate
import org.rstorrent.session.uniffi.ViewUpdatePayload

class ProductEngineService : Service() {
    inner class LocalBinder : Binder() {
        val service: ProductEngineService
            get() = this@ProductEngineService
    }

    private val binder = LocalBinder()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val requestPrefix = UUID.randomUUID().toString()
    private val requestIds = AtomicLong(1)
    private val stopped = AtomicBoolean(false)
    private val clientReady = CompletableDeferred<Unit>()
    private val presentationReady = CompletableDeferred<Unit>()
    private val mutableState = MutableStateFlow(ProductState())
    val state: StateFlow<ProductState> = mutableState.asStateFlow()

    private lateinit var client: AndroidApplicationClient
    private lateinit var presentationRepository: AndroidPresentationRepository
    private var trackerEvidenceSubscription: AndroidViewSubscription? = null
    private var trackerEvidenceJob: Job? = null
    @Volatile private var safStorageJobs: List<Job> = emptyList()
    @Volatile private var safTreeUri: Uri? = null
    private val safWork = ConcurrentHashMap.newKeySet<String>()
    private val crashAfterSafRename = AtomicBoolean(false)
    private var powerLock: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, notification("Opening profile"))
        safTreeUri = ProductSafDocuments.selectedTree(this)
        mutableState.update {
            it.copy(
                storageRootReady = false,
                storageRootLabel = safTreeUri?.lastPathSegment,
            )
        }
        scope.launch {
            try {
                PlatformTrustBootstrap.ensureInitialized(applicationContext)
                val profile = File(filesDir, "product-profile")
                check(profile.mkdirs() || profile.isDirectory)
                client =
                    AndroidApplicationClient.open(
                        AndroidApplicationConfig(
                            profile.absolutePath,
                            "default",
                            "",
                            true,
                            AndroidNetworkPolicy.ONLINE,
                            15UL,
                            60UL,
                        ),
                    )
                presentationRepository =
                    AndroidPresentationRepository(
                        scope,
                        mutableState,
                        stopped,
                        onUpdate = { update, product, driveSaf ->
                            traceUpdate(update, product)
                            if (driveSaf) advanceSaf(product)
                        },
                        onError = ::reportError,
                    )
                presentationRepository.start(client)
                presentationReady.complete(Unit)
                safStorageJobs =
                    List(SAF_PROVIDER_CONCURRENCY) {
                        scope.launch(Dispatchers.IO) { driveSafStorageRequests() }
                    }
                val storageRootHealthy = client.probeSafStorageRoots()
                Log.i(TAG, "saf_root_health source=startup available=$storageRootHealthy")
                mutableState.update {
                    it.copy(
                        ready = true,
                        storageRootReady = storageRootHealthy,
                        error =
                            if (safTreeUri != null && !storageRootHealthy) {
                                "Selected download folder is unavailable"
                            } else {
                                null
                            },
                    )
                }
                clientReady.complete(Unit)
                observePowerAndNotification()
            } catch (error: Throwable) {
                if (!clientReady.isCompleted) {
                    clientReady.completeExceptionally(error)
                }
                if (!presentationReady.isCompleted) {
                    presentationReady.completeExceptionally(error)
                }
                Log.e(TAG, "product service initialization failed", error)
                mutableState.update {
                    it.copy(ready = false, error = error.message ?: error.toString())
                }
                updateNotification("Engine unavailable")
            }
        }
    }

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        if (intent?.action == ACTION_STOP) {
            scope.launch {
                shutdown()
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        runBlocking(Dispatchers.IO) {
            shutdown()
        }
        scope.cancel()
        super.onDestroy()
    }

    fun addMagnet(
        magnet: String,
        skipFiles: List<UInt> = emptyList(),
        startContent: Boolean = true,
    ) {
        if (safTreeUri == null) {
            mutableState.update { it.copy(error = "Select a download folder first") }
            return
        }
        scope.launch {
            try {
                clientReady.await()
                dispatchAddAwait(
                    Command.AddMagnet(magnet.trim(), "downloads", startContent, skipFiles),
                    magnetV1(magnet),
                )
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun addTorrentFile(
        uri: Uri,
        startContent: Boolean = true,
    ) {
        if (safTreeUri == null) {
            mutableState.update { it.copy(error = "Select a download folder first") }
            return
        }
        scope.launch(Dispatchers.IO) {
            try {
                clientReady.await()
                val source = readTorrentSource(uri)
                dispatchTorrentSource(source, startContent)
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun addTorrentBytes(
        source: ByteArray,
        startContent: Boolean = true,
    ) {
        if (safTreeUri == null) {
            mutableState.update { it.copy(error = "Select a download folder first") }
            return
        }
        scope.launch(Dispatchers.IO) {
            try {
                clientReady.await()
                require(source.isNotEmpty()) { "Torrent file is empty" }
                require(source.size <= MAX_TORRENT_SOURCE_BYTES) {
                    "Torrent file exceeds the ${MAX_TORRENT_SOURCE_BYTES / (1024 * 1024)} MiB limit"
                }
                dispatchTorrentSource(source, startContent)
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    private suspend fun dispatchTorrentSource(
        source: ByteArray,
        startContent: Boolean,
    ) {
        val request =
            AddTorrentBytesRequest(
                version = 1U.toUShort(),
                requestId = "android-$requestPrefix-${requestIds.getAndIncrement()}",
                expectedRevision = null,
                storageRoot = "downloads",
                startContent = startContent,
                selection = FileSelectionIntent.All,
                sourceLength = source.size.toUInt(),
            )
        val response = client.addTorrentBytes(request, source)
        val outcome = response.outcome
        if (outcome is ResponseOutcome.Error) error(outcome.error.message)
        logAddResult(response, null)
    }

    private fun readTorrentSource(uri: Uri): ByteArray {
        val input = contentResolver.openInputStream(uri) ?: error("Unable to open torrent file")
        return input.use { stream ->
            val output = ByteArrayOutputStream()
            val buffer = ByteArray(16 * 1024)
            var total = 0
            while (true) {
                val count = stream.read(buffer)
                if (count < 0) break
                total += count
                require(total <= MAX_TORRENT_SOURCE_BYTES) {
                    "Torrent file exceeds the ${MAX_TORRENT_SOURCE_BYTES / (1024 * 1024)} MiB limit"
                }
                output.write(buffer, 0, count)
            }
            require(total > 0) { "Torrent file is empty" }
            output.toByteArray()
        }
    }

    fun addMagnetWithTrackerPolicyForTest(
        magnet: String,
        policyName: String,
        startContent: Boolean,
    ) {
        check(ProductSafDocuments.isDebuggable(this)) {
            "tracker authentication injection is debug-only"
        }
        if (safTreeUri == null) {
            mutableState.update { it.copy(error = "Select a download folder first") }
            return
        }
        val policy =
            when (policyName) {
                "system_trust" -> HttpsServerAuthenticationPolicy.SYSTEM_TRUST
                "disabled" -> HttpsServerAuthenticationPolicy.DISABLED
                else -> error("unknown tracker HTTPS authentication policy")
            }
        val v1InfoHash = magnetV1(magnet)
        scope.launch {
            try {
                clientReady.await()
                dispatchAwait(
                    Command.SetClientSettings(
                        ClientSettings(
                            listener = ListenerPolicy.Disabled,
                            preferredListenPort = 6_881U.toUShort(),
                            portMapping = PortMappingPolicy.DISABLED,
                            peerConnectionLimit = 200U,
                            uploadSlots = 8U.toUShort(),
                            activeDownloads = 3U.toUShort(),
                            uploadRateLimit = TransferRateLimit.Unlimited,
                            downloadRateLimit = TransferRateLimit.Unlimited,
                            encryption = EncryptionPolicy.ALLOW,
                            ipv6Enabled = true,
                            trackerHttpsServerAuthentication = policy,
                        ),
                    ),
                )
                awaitTrackerPolicy(policy)
                val torrentId = dispatchAddAwait(
                    Command.AddMagnet(magnet.trim(), "downloads", startContent, emptyList()),
                    v1InfoHash,
                )
                subscribeTrackerEvidenceForTest(torrentId)
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun addMagnetWithEncryptionPolicyForTest(
        magnet: String,
        policyName: String,
        skipFiles: List<UInt> = emptyList(),
    ) {
        check(ProductSafDocuments.isDebuggable(this)) {
            "peer encryption injection is debug-only"
        }
        if (safTreeUri == null) {
            mutableState.update { it.copy(error = "Select a download folder first") }
            return
        }
        val policy =
            when (policyName) {
                "disabled" -> EncryptionPolicy.DISABLED
                "allow" -> EncryptionPolicy.ALLOW
                "prefer" -> EncryptionPolicy.PREFER
                "required" -> EncryptionPolicy.REQUIRED
                else -> error("unknown peer encryption policy")
            }
        scope.launch {
            try {
                clientReady.await()
                dispatchAwait(
                    Command.SetClientSettings(
                        ClientSettings(
                            listener = ListenerPolicy.Disabled,
                            preferredListenPort = 6_881U.toUShort(),
                            portMapping = PortMappingPolicy.DISABLED,
                            peerConnectionLimit = 200U,
                            uploadSlots = 8U.toUShort(),
                            activeDownloads = 3U.toUShort(),
                            uploadRateLimit = TransferRateLimit.Unlimited,
                            downloadRateLimit = TransferRateLimit.Unlimited,
                            encryption = policy,
                            ipv6Enabled = true,
                            trackerHttpsServerAuthentication =
                                HttpsServerAuthenticationPolicy.SYSTEM_TRUST,
                        ),
                    ),
                )
                awaitEncryptionPolicy(policy)
                dispatchAddAwait(
                    Command.AddMagnet(magnet.trim(), "downloads", true, skipFiles),
                    magnetV1(magnet),
                )
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun logMseDhEvidenceForTest() {
        check(ProductSafDocuments.isDebuggable(this)) {
            "MSE DH evidence is debug-only"
        }
        scope.launch {
            try {
                clientReady.await()
                val snapshot = client.mseDhWorkSnapshot()
                Log.i(
                    TAG,
                    "mse_dh_work waiting=${snapshot.waiting} active=${snapshot.active} " +
                        "high_water=${snapshot.highWater} tracked=${snapshot.tracked} " +
                        "closed=${snapshot.closed}",
                )
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun logDownloadAdmissionEvidenceForTest(mode: String) {
        check(ProductSafDocuments.isDebuggable(this)) {
            "download admission evidence is debug-only"
        }
        scope.launch {
            try {
                clientReady.await()
                val expectedActive =
                    when (mode) {
                        "active" -> 2U.toUShort()
                        "terminal" -> 0U.toUShort()
                        else -> error("unknown download admission evidence mode")
                    }
                val settings = awaitDownloadAdmission(expectedActive)
                val queued =
                    withTimeout(10_000) {
                        while (true) {
                            val count =
                                mutableState.value.torrents.values.count {
                                    it.operationalState.name == "QUEUED"
                                }
                            if ((mode == "active" && count >= 1) || (mode == "terminal" && count == 0)) {
                                return@withTimeout count
                            }
                            delay(25)
                        }
                        error("unreachable")
                    }
                val resources =
                    withTimeout(10_000) {
                        while (true) {
                            val snapshot = client.downloadResourceSnapshot()
                            if (mode != "terminal" || snapshot.registeredGenerations == 0UL) {
                                return@withTimeout snapshot
                            }
                            delay(25)
                        }
                        error("unreachable")
                    }
                Log.i(
                    TAG,
                    "download_admission mode=$mode " +
                        "configured=${settings.configured.activeDownloads} " +
                        "effective=${settings.effectiveActiveDownloads} " +
                        "active=${settings.activeDownloadCount} queued=$queued " +
                        "registered=${resources.registeredGenerations} " +
                        "registered_high=${resources.registeredGenerationsHighWater} " +
                        "request_high=${resources.outstandingRequestHighWater} " +
                        "payload_high=${resources.bufferedPayloadHighWater} " +
                        "piece_bytes_high=${resources.activePieceBytesHighWater} " +
                        "pieces_high=${resources.activePiecesHighWater} " +
                        "writes_high=${resources.activeStorageWritesHighWater} " +
                        "hashes_high=${resources.activeStorageHashesHighWater}",
                )
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    private suspend fun awaitDownloadAdmission(active: UShort): ClientSettingsRuntimeView {
        val subscription =
            client.subscribe(
                SubscriptionSpec(
                    ViewSelector.TorrentList,
                    ViewProjection.SUMMARY,
                    DeliveryPolicy(0U, 256U * 1024U),
                    null,
                    null,
                ),
            )
        try {
            return withTimeout(10_000) {
                while (true) {
                    val update = subscription.nextUpdate() ?: error("settings view closed")
                    val settings =
                        when (val payload = update.payload) {
                            is ViewUpdatePayload.Snapshot ->
                                (payload.snapshot as? ViewSnapshot.TorrentList)?.clientSettings
                            is ViewUpdatePayload.Patch ->
                                (payload.patch as? ViewPatch.TorrentList)?.clientSettings
                            is ViewUpdatePayload.ResetRequired -> {
                                subscription.resync()
                                null
                            }
                        }
                    if (settings != null && settings.activeDownloadCount == active) {
                        return@withTimeout settings
                    }
                }
                error("unreachable")
            }
        } finally {
            subscription.close()
        }
    }

    fun exerciseIpv6PolicyForTest(mode: String) {
        check(ProductSafDocuments.isDebuggable(this)) {
            "IPv6 policy evidence is debug-only"
        }
        Log.i(TAG, "ipv6_settings_begin mode=$mode")
        scope.launch {
            try {
                clientReady.await()
                val current = awaitIpv6Policy(null)
                if (mode == "disable_sequence") {
                    logIpv6Evidence("initial", current)
                    dispatchAwait(
                        Command.SetClientSettings(
                            current.configured.copy(ipv6Enabled = false),
                        ),
                    )
                    logIpv6Evidence("disabled", awaitIpv6Policy(false))
                    return@launch
                }
                if (mode == "enable_sequence") {
                    logIpv6Evidence("restarted", current)
                    dispatchAwait(
                        Command.SetClientSettings(
                            current.configured.copy(ipv6Enabled = true),
                        ),
                    )
                    logIpv6Evidence("reenabled", awaitIpv6Policy(true))
                    return@launch
                }
                val desired =
                    when (mode) {
                        "observe" -> null
                        "disable" -> false
                        "enable" -> true
                        else -> error("unknown IPv6 policy evidence mode")
                    }
                val observed =
                    if (desired == null) {
                        current
                    } else {
                        dispatchAwait(
                            Command.SetClientSettings(
                                current.configured.copy(ipv6Enabled = desired),
                            ),
                        )
                        awaitIpv6Policy(desired)
                    }
                logIpv6Evidence(mode, observed)
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun exerciseBandwidthPolicyForTest(mode: String) {
        check(ProductSafDocuments.isDebuggable(this)) {
            "bandwidth policy evidence is debug-only"
        }
        scope.launch {
            try {
                clientReady.await()
                if (mode == "configure") {
                    val current = awaitIpv6Policy(null)
                    val limit = TransferRateLimit.Limited(ANDROID_RATE_BYTES_PER_SECOND.toUInt())
                    dispatchAwait(
                        Command.SetClientSettings(
                            current.configured.copy(
                                uploadRateLimit = limit,
                                downloadRateLimit = limit,
                            ),
                        ),
                    )
                }
                logBandwidthEvidence(mode, awaitBandwidthPolicy(mode))
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    private suspend fun awaitBandwidthPolicy(mode: String): ClientSettingsRuntimeView =
        withTimeout(30_000) {
            while (true) {
                val settings = mutableState.value.clientSettings
                val configured =
                    (settings?.configured?.downloadRateLimit as? TransferRateLimit.Limited)
                        ?.bytesPerSecond
                val effective =
                    (settings?.effectiveDownloadRateLimit as? TransferRateLimit.Limited)
                        ?.bytesPerSecond
                val download = settings?.bandwidth?.download
                val policyApplied =
                    configured == ANDROID_RATE_BYTES_PER_SECOND.toUInt() &&
                        effective == ANDROID_RATE_BYTES_PER_SECOND.toUInt() &&
                        settings?.bandwidthApplication is ClientSettingsApplicationState.Applied
                val ready =
                    when (mode) {
                        "configured", "configure" -> policyApplied
                        "active" ->
                            policyApplied &&
                                settings?.activeDownloadCount == 2U.toUShort() &&
                                download != null &&
                                download.grantedBytes.toULong() > 0UL &&
                                download.throttleWaitHighWaterMicros.toULong() > 0UL
                        "terminal" ->
                            policyApplied &&
                                settings?.activeDownloadCount == 0U.toUShort() &&
                                download != null &&
                                download.grantedBytes.toULong() > 0UL &&
                                download.activeWaiters == 0U &&
                                download.queuedRequestedBytes.toULong() == 0UL
                        else -> error("unknown bandwidth policy evidence mode")
                    }
                if (ready) return@withTimeout requireNotNull(settings)
                delay(25)
            }
            error("unreachable")
        }

    private fun logBandwidthEvidence(
        mode: String,
        settings: ClientSettingsRuntimeView,
    ) {
        val configured =
            (settings.configured.downloadRateLimit as TransferRateLimit.Limited).bytesPerSecond
        val effective =
            (settings.effectiveDownloadRateLimit as TransferRateLimit.Limited).bytesPerSecond
        val download = settings.bandwidth.download
        Log.i(
            TAG,
            "bandwidth_policy mode=$mode configured=$configured effective=$effective " +
                "application=APPLIED registered=${download.registeredTorrents} " +
                "active_downloads=${settings.activeDownloadCount} " +
                "active_waiters=${download.activeWaiters} " +
                "queued=${download.queuedRequestedBytes} granted=${download.grantedBytes} " +
                "returned=${download.returnedBytes} " +
                "wait_high=${download.throttleWaitHighWaterMicros} " +
                "burst=${download.currentBurstCreditBytes}",
        )
    }

    private fun logIpv6Evidence(mode: String, observed: ClientSettingsRuntimeView) {
        val application =
            when (observed.ipv6Application) {
                is ClientSettingsApplicationState.Applied -> "APPLIED"
                is ClientSettingsApplicationState.Applying -> "APPLYING"
                is ClientSettingsApplicationState.Degraded -> "DEGRADED"
            }
        val ipv6 = observed.transportFamilies.firstOrNull { it.family.name == "IPV6" }
        Log.i(
            TAG,
            "ipv6_settings mode=$mode configured=${observed.configured.ipv6Enabled} " +
                "effective=${observed.effectiveIpv6Enabled} application=$application " +
                "tcp=${ipv6?.tcpEndpoint ?: "none"} udp=${ipv6?.udpEndpoint ?: "none"}",
        )
    }

    private suspend fun awaitIpv6Policy(configured: Boolean?): ClientSettingsRuntimeView {
        val subscription =
            client.subscribe(
                SubscriptionSpec(
                    ViewSelector.TorrentList,
                    ViewProjection.SUMMARY,
                    DeliveryPolicy(0U, 256U * 1024U),
                    null,
                    null,
                ),
            )
        try {
            return withTimeout(10_000) {
                while (true) {
                    val update = subscription.nextUpdate() ?: error("settings view closed")
                    val settings =
                        when (val payload = update.payload) {
                            is ViewUpdatePayload.Snapshot ->
                                (payload.snapshot as? ViewSnapshot.TorrentList)?.clientSettings
                            is ViewUpdatePayload.Patch ->
                                (payload.patch as? ViewPatch.TorrentList)?.clientSettings
                            is ViewUpdatePayload.ResetRequired -> {
                                subscription.resync()
                                null
                            }
                        }
                    if (
                        settings != null &&
                        (configured == null || settings.configured.ipv6Enabled == configured) &&
                        settings.ipv6Application !is ClientSettingsApplicationState.Applying
                    ) {
                        return@withTimeout settings
                    }
                }
                error("unreachable")
            }
        } finally {
            subscription.close()
        }
    }

    fun setSafTree(treeUri: Uri) {
        scope.launch {
            try {
                clientReady.await()
                val restart = client.prepareSafTreeReplacement()
                safTreeUri = treeUri
                val storageRootHealthy = client.probeSafStorageRoots()
                Log.i(TAG, "saf_root_health source=selection available=$storageRootHealthy")
                Log.i(TAG, "saf_tree_ready uri=$treeUri")
                mutableState.update {
                    it.copy(
                        storageRootReady = storageRootHealthy,
                        storageRootLabel = treeUri.lastPathSegment,
                        error =
                            if (storageRootHealthy) {
                                null
                            } else {
                                "Selected download folder is unavailable"
                            },
                    )
                }
                if (!storageRootHealthy) return@launch
                advanceSaf(mutableState.value)
                mutableState.value.torrents.values
                    .filter { it.state == TorrentState.AWAITING_STORAGE }
                    .forEach { resume(it.torrentId) }
                restart?.let(::resume)
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun enableCrashAfterSafRenameForTest() {
        check(ProductSafDocuments.isDebuggable(this)) {
            "SAF publication crash injection is debug-only"
        }
        crashAfterSafRename.set(true)
    }

    fun exerciseTorrentActionForTest(
        torrentId: String,
        action: String,
    ) {
        check(ProductSafDocuments.isDebuggable(this)) {
            "torrent lifecycle injection is debug-only"
        }
        scope.launch {
            try {
                clientReady.await()
                when (action) {
                    "pause" -> dispatchAwait(Command.Pause(torrentId))
                    "resume" -> dispatchAwait(Command.Resume(torrentId))
                    "force_recheck" -> forceRecheckAndAwaitForTest(torrentId)
                    "remove" ->
                        dispatchAwait(
                            Command.RemoveTorrent(
                                torrentId,
                                RemovalDataPolicy.DELETE_MANAGED,
                            ),
                        )
                    "enable_upload" -> {
                        val current = awaitIpv6Policy(null)
                        dispatchAwait(
                            Command.SetClientSettings(
                                current.configured.copy(
                                    listener = ListenerPolicy.FixedLocalNetwork(6_881U.toUShort()),
                                    preferredListenPort = 6_881U.toUShort(),
                                    portMapping = PortMappingPolicy.DISABLED,
                                ),
                            ),
                        )
                        awaitFixedListener(6_881U.toUShort())
                    }
                    else -> error("unknown torrent lifecycle action")
                }
                Log.i(TAG, "torrent_action_completed torrent=$torrentId action=$action")
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    private suspend fun awaitFixedListener(port: UShort): ClientSettingsRuntimeView {
        val subscription =
            client.subscribe(
                SubscriptionSpec(
                    ViewSelector.TorrentList,
                    ViewProjection.SUMMARY,
                    DeliveryPolicy(0U, 256U * 1024U),
                    null,
                    null,
                ),
            )
        try {
            return withTimeout(10_000) {
                while (true) {
                    val update = subscription.nextUpdate() ?: error("settings view closed")
                    val settings =
                        when (val payload = update.payload) {
                            is ViewUpdatePayload.Snapshot ->
                                (payload.snapshot as? ViewSnapshot.TorrentList)?.clientSettings
                            is ViewUpdatePayload.Patch ->
                                (payload.patch as? ViewPatch.TorrentList)?.clientSettings
                            is ViewUpdatePayload.ResetRequired -> {
                                subscription.resync()
                                null
                            }
                        }
                    val listener = settings?.listenerStatus
                    if (listener is ListenerStatus.Listening && listener.port == port) {
                        return@withTimeout settings
                    }
                    if (listener is ListenerStatus.BindFailed) {
                        error("fixed upload listener failed: ${listener.detail}")
                    }
                }
                error("unreachable")
            }
        } finally {
            subscription.close()
        }
    }

    private suspend fun forceRecheckAndAwaitForTest(torrentId: String) {
        val subscription =
            client.subscribe(
                SubscriptionSpec(
                    ViewSelector.Torrent(torrentId),
                    ViewProjection.SUMMARY,
                    DeliveryPolicy(0U, 256U * 1024U),
                    null,
                    null,
                ),
            )
        try {
            dispatchAwait(Command.ForceRecheck(torrentId))
            withTimeout(10_000) {
                var sawChecking = false
                while (true) {
                    val update = subscription.nextUpdate() ?: error("torrent view closed")
                    val torrent =
                        when (val payload = update.payload) {
                            is ViewUpdatePayload.Snapshot ->
                                (payload.snapshot as? ViewSnapshot.Torrent)?.torrent
                            is ViewUpdatePayload.Patch ->
                                (payload.patch as? ViewPatch.Torrent)?.torrent
                            is ViewUpdatePayload.ResetRequired -> {
                                subscription.resync()
                                null
                            }
                        }
                    when (torrent?.state) {
                        TorrentState.CHECKING -> sawChecking = true
                        TorrentState.COMPLETE -> if (sawChecking) return@withTimeout
                        else -> {}
                    }
                }
            }
        } finally {
            subscription.close()
        }
    }

    fun pause(torrentId: String) {
        dispatch(Command.Pause(torrentId))
    }

    fun resume(torrentId: String) {
        dispatch(Command.Resume(torrentId))
    }

    fun forceRecheck(torrentId: String) {
        dispatch(Command.ForceRecheck(torrentId))
    }

    fun moveDownloadToTop(torrentId: String) {
        dispatch(Command.MoveDownloadToTop(torrentId))
    }

    fun moveDownloadToBottom(torrentId: String) {
        dispatch(Command.MoveDownloadToBottom(torrentId))
    }

    fun archive(torrentId: String) {
        dispatch(Command.Archive(torrentId))
    }

    fun restoreArchive(torrentId: String) {
        dispatch(Command.RestoreArchive(torrentId))
    }

    fun removeTorrent(
        torrentId: String,
        policy: RemovalDataPolicy,
    ) {
        dispatch(Command.RemoveTorrent(torrentId, policy))
    }

    fun setFileWanted(
        torrentId: String,
        fileIndex: UInt,
        wanted: Boolean,
    ) {
        dispatch(
            Command.SetFilePriority(
                torrentId,
                listOf(fileIndex),
                if (wanted) FilePriority.NORMAL else FilePriority.SKIP,
            ),
        )
    }

    fun downloadFileNow(
        torrentId: String,
        fileIndex: UInt,
    ) {
        dispatch(Command.DownloadFiles(torrentId, listOf(fileIndex)))
    }

    fun openCompletedFile(
        torrentName: String,
        file: FileView,
    ) {
        scope.launch(Dispatchers.IO) {
            try {
                require(file.verifiedBytes == file.lengthBytes) { "File is not complete" }
                val tree = safTreeUri ?: error("Download folder is unavailable")
                val path =
                    if (file.path.firstOrNull() == torrentName) {
                        file.path
                    } else {
                        listOf(torrentName) + file.path
                    }
                val document =
                    ProductSafDocuments.publishedDocument(this@ProductEngineService, tree, path)
                        ?: error("Published file is unavailable")
                val mime = contentResolver.getType(document) ?: "application/octet-stream"
                startActivity(
                    Intent(Intent.ACTION_VIEW).apply {
                        setDataAndType(document, mime)
                        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    },
                )
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun updateClientSettings(transform: (ClientSettings) -> ClientSettings) {
        val configured = mutableState.value.clientSettings?.configured
        if (configured == null) {
            mutableState.update { it.copy(error = "Settings are still loading") }
            return
        }
        dispatch(Command.SetClientSettings(transform(configured)))
    }

    fun setTorrentTransferLimits(
        torrentId: String,
        limits: TorrentTransferLimits,
    ) {
        dispatch(Command.SetTorrentTransferLimits(torrentId, limits))
    }

    fun copyMagnet(torrentId: String) {
        scope.launch {
            try {
                clientReady.await()
                val response = dispatchForResponse(Command.ExportMagnet(torrentId))
                val result = response.result as? CommandResult.ExportMagnet
                    ?: error("Magnet export returned no value")
                val clipboard = getSystemService(ClipboardManager::class.java)
                clipboard.setPrimaryClip(ClipData.newPlainText("Magnet link", result.result.magnet))
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun shutdownFromUi() {
        scope.launch {
            try {
                clientReady.await()
                shutdown()
            } finally {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
        }
    }

    fun selectTorrent(torrentId: String) {
        withPresentation { it.selectTorrent(torrentId) }
    }

    fun presentTorrent(
        torrentId: String,
        presentation: TorrentPresentation,
    ) {
        withPresentation { it.presentTorrent(torrentId, presentation) }
    }

    fun clearTorrentPresentation(torrentId: String) {
        withPresentation { it.clearTorrent(torrentId) }
    }

    fun presentCatalogPage(
        torrentId: String,
        presentation: TorrentPresentation,
        offset: UInt,
    ) {
        withPresentation { it.presentCatalogPage(torrentId, presentation, offset) }
    }

    fun presentGlobal(presentation: GlobalPresentation) {
        withPresentation { it.presentGlobal(presentation) }
    }

    fun configureDiagnostics(
        profile: DiagnosticProfile,
        severity: DiagnosticSeverity,
        categories: List<DiagnosticCategory>,
        torrentOnly: Boolean,
    ) {
        withPresentation { it.configureDiagnostics(profile, severity, categories, torrentOnly) }
    }

    private fun withPresentation(action: (AndroidPresentationRepository) -> Unit) {
        scope.launch {
            try {
                presentationReady.await()
                action(presentationRepository)
            } catch (error: Throwable) {
                if (!stopped.get() && error !is CancellationException) reportError(error)
            }
        }
    }

    private fun dispatch(command: Command) {
        scope.launch {
            try {
                clientReady.await()
                dispatchAwait(command)
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    private suspend fun dispatchAwait(command: Command) {
        dispatchForResponse(command)
    }

    private fun magnetV1(magnet: String): String =
        Regex("(?i)urn:btih:([0-9a-f]{40})")
            .find(magnet)
            ?.groupValues
            ?.get(1)
            ?.lowercase()
            ?: error("test magnet has no hexadecimal v1 identity")

    private suspend fun dispatchAddAwait(
        command: Command,
        v1InfoHash: String?,
    ): String = logAddResult(dispatchForResponse(command), v1InfoHash)

    private fun logAddResult(
        response: org.rstorrent.session.uniffi.ResponseEnvelope,
        v1InfoHash: String?,
    ): String {
        val add = (response.result as? CommandResult.AddTorrent)?.result
            ?: error("add response omitted its result")
        Log.i(
            TAG,
            "torrent_added torrent=${add.torrentId} " +
                "protocol_v1=${v1InfoHash ?: "unknown"} " +
                "disposition=${add.disposition::class.simpleName}",
        )
        return add.torrentId
    }

    private suspend fun dispatchForResponse(command: Command): org.rstorrent.session.uniffi.ResponseEnvelope {
        val response =
            client.dispatch(
                RequestEnvelope(
                    1U.toUShort(),
                    "android-$requestPrefix-${requestIds.getAndIncrement()}",
                    null,
                    command,
                ),
            )
        val outcome = response.outcome
        if (outcome is ResponseOutcome.Error) {
            error(outcome.error.message)
        }
        return response
    }

    private suspend fun awaitTrackerPolicy(policy: HttpsServerAuthenticationPolicy) {
        val subscription =
            client.subscribe(
                SubscriptionSpec(
                    ViewSelector.TorrentList,
                    ViewProjection.SUMMARY,
                    DeliveryPolicy(0U, 256U * 1024U),
                    null,
                    null,
                ),
            )
        try {
            withTimeout(10_000) {
                while (true) {
                    val update = subscription.nextUpdate() ?: error("settings view closed")
                    val settings =
                        when (val payload = update.payload) {
                            is ViewUpdatePayload.Snapshot ->
                                (payload.snapshot as? ViewSnapshot.TorrentList)?.clientSettings
                            is ViewUpdatePayload.Patch ->
                                (payload.patch as? ViewPatch.TorrentList)?.clientSettings
                            is ViewUpdatePayload.ResetRequired -> {
                                subscription.resync()
                                null
                            }
                        }
                    if (
                        settings?.configured?.trackerHttpsServerAuthentication == policy &&
                        settings.effectiveTrackerHttpsServerAuthentication == policy &&
                        settings.trackerHttpsAuthenticationApplication is
                            ClientSettingsApplicationState.Applied
                    ) {
                        Log.i(
                            TAG,
                            "tracker_https_settings configured=$policy effective=$policy " +
                                "application=APPLIED",
                        )
                        return@withTimeout
                    }
                }
            }
        } finally {
            subscription.close()
        }
    }

    private suspend fun awaitEncryptionPolicy(policy: EncryptionPolicy) {
        val subscription =
            client.subscribe(
                SubscriptionSpec(
                    ViewSelector.TorrentList,
                    ViewProjection.SUMMARY,
                    DeliveryPolicy(0U, 256U * 1024U),
                    null,
                    null,
                ),
            )
        try {
            withTimeout(10_000) {
                while (true) {
                    val update = subscription.nextUpdate() ?: error("settings view closed")
                    val settings =
                        when (val payload = update.payload) {
                            is ViewUpdatePayload.Snapshot ->
                                (payload.snapshot as? ViewSnapshot.TorrentList)?.clientSettings
                            is ViewUpdatePayload.Patch ->
                                (payload.patch as? ViewPatch.TorrentList)?.clientSettings
                            is ViewUpdatePayload.ResetRequired -> {
                                subscription.resync()
                                null
                            }
                        }
                    if (
                        settings?.configured?.encryption == policy &&
                        settings.effectiveEncryption == policy &&
                        settings.encryptionApplication is ClientSettingsApplicationState.Applied
                    ) {
                        Log.i(
                            TAG,
                            "mse_settings configured=$policy effective=$policy application=APPLIED",
                        )
                        return@withTimeout
                    }
                }
            }
        } finally {
            subscription.close()
        }
    }

    fun subscribeTrackerEvidenceForTest(torrentId: String) {
        check(ProductSafDocuments.isDebuggable(this)) {
            "tracker evidence subscription is debug-only"
        }
        trackerEvidenceJob?.cancel()
        trackerEvidenceSubscription?.close()
        trackerEvidenceJob = null
        trackerEvidenceSubscription = null
        trackerEvidenceJob =
            scope.launch {
                val subscription =
                    client.subscribe(
                        SubscriptionSpec(
                            ViewSelector.Torrent(torrentId),
                            ViewProjection.TRACKERS,
                            DeliveryPolicy(0U, 256U * 1024U),
                            null,
                            CatalogPageRequest(0U, 1_024U),
                        ),
                    )
                trackerEvidenceSubscription = subscription
                try {
                    while (true) {
                        val update = subscription.nextUpdate() ?: break
                        val trackers =
                            when (val payload = update.payload) {
                                is ViewUpdatePayload.Snapshot ->
                                    (payload.snapshot as? ViewSnapshot.Trackers)?.trackers.orEmpty()
                                is ViewUpdatePayload.Patch ->
                                    (payload.patch as? ViewPatch.Trackers)?.upsert.orEmpty()
                                is ViewUpdatePayload.ResetRequired -> {
                                    subscription.resync()
                                    emptyList()
                                }
                            }
                        trackers.forEach { tracker ->
                            Log.i(
                                TAG,
                                "tracker_evidence torrent=$torrentId security=${tracker.security} " +
                                    "status=${tracker.status} attempts=${tracker.totalAttempts} " +
                                    "failures=${tracker.consecutiveFailures} " +
                                    "peer_count=${tracker.lastPeerCount ?: -1} " +
                                    "error=${tracker.lastError != null} " +
                                    "error_detail=${tracker.lastError ?: "none"}",
                            )
                        }
                    }
                } catch (error: Throwable) {
                    if (!stopped.get() && error !is CancellationException) reportError(error)
                } finally {
                    subscription.close()
                }
            }
    }

    private fun advanceSaf(product: ProductState) {
        val treeUri = safTreeUri ?: return
        for (torrent in product.torrents.values) {
            val action =
                if (torrent.removalState == RemovalState.AWAITING_PLATFORM) {
                    "removal"
                } else {
                    when (torrent.state) {
                        TorrentState.AWAITING_PUBLICATION -> "publication"
                        else -> continue
                    }
                }
            val key = "${torrent.torrentId}:$action"
            if (!safWork.add(key)) continue
            scope.launch {
                try {
                    when (action) {
                        "publication" -> {
                            Log.i(TAG, "saf_publication_begin torrent=${torrent.torrentId}")
                            check(client.preparedSafFiles(torrent.torrentId).isNotEmpty()) {
                                "native prepared publication manifest is empty"
                            }
                            val name = client.prepareDynamicSafPublication(torrent.torrentId)
                            ProductSafDocuments.publish(
                                this@ProductEngineService,
                                treeUri,
                                torrent.torrentId,
                                name,
                            )
                            if (crashAfterSafRename.compareAndSet(true, false)) {
                                Log.i(
                                    TAG,
                                    "saf_test_crash_after_rename torrent=${torrent.torrentId}",
                                )
                                android.os.Process.killProcess(android.os.Process.myPid())
                                error("process survived SAF publication crash injection")
                            }
                            client.confirmDynamicSafPublication(torrent.torrentId)
                            val storageMetrics = client.safStoragePoolSnapshot()
                            Log.i(
                                TAG,
                                "saf_storage_metrics torrent=${torrent.torrentId} " +
                                    "limit=${storageMetrics.limit} " +
                                    "owned_high_water=${storageMetrics.ownedHighWater} " +
                                    "pending_high_water=${storageMetrics.platformPendingHighWater}",
                            )
                            Log.i(TAG, "saf_publication_confirmed torrent=${torrent.torrentId}")
                        }
                        "removal" -> {
                            Log.i(TAG, "saf_removal_begin torrent=${torrent.torrentId}")
                            val plan = client.safRemovalPlan(torrent.torrentId)
                            ProductSafDocuments.deleteManaged(
                                this@ProductEngineService,
                                treeUri,
                                plan,
                            )
                            client.confirmSafRemoval(torrent.torrentId, plan.operationId)
                            Log.i(TAG, "saf_removal_confirmed torrent=${torrent.torrentId}")
                        }
                        else -> error("unknown SAF action $action")
                    }
                } catch (error: Throwable) {
                    if (action == "removal") {
                        try {
                            val plan = client.safRemovalPlan(torrent.torrentId)
                            client.failSafRemoval(
                                torrent.torrentId,
                                plan.operationId,
                                error.message ?: error.toString(),
                            )
                        } catch (markError: Throwable) {
                            error.addSuppressed(markError)
                        }
                    }
                    reportError(error)
                } finally {
                    safWork.remove(key)
                }
            }
        }
    }

    private suspend fun driveSafStorageRequests() {
        while (!stopped.get()) {
            val request = client.nextSafStorageRequest() ?: return
            val cancellation = CancellationSignal()
            try {
                withTimeout(request.timeoutMillis.toLong()) {
                    val treeUri =
                        ProductSafDocuments.selectedTree(this@ProductEngineService)
                            ?: throw SafStorageRequestException(
                                SafStorageFailureKind.GRANT_UNAVAILABLE,
                                "persisted SAF grant is unavailable",
                            )
                    when (request.operation) {
                        SafStorageOperation.DELETE -> {
                            ProductSafDocuments.deleteDynamic(
                                this@ProductEngineService,
                                treeUri,
                                request,
                            )
                            client.completeSafStorageDelete(request.requestId)
                        }
                        SafStorageOperation.OBSERVE -> {
                            val observation =
                                ProductSafDocuments.observeDynamic(
                                    this@ProductEngineService,
                                    treeUri,
                                    request,
                                )
                            client.completeSafStorageObservation(request.requestId, observation)
                        }
                        SafStorageOperation.OPEN -> {
                            ProductSafDocuments
                                .openDynamic(
                                    this@ProductEngineService,
                                    treeUri,
                                    request,
                                    cancellation,
                                ).use { descriptor ->
                                    client.completeSafStorageRequest(
                                        request.requestId,
                                        descriptor.fd,
                                        request.access,
                                    )
                                }
                        }
                    }
                }
            } catch (error: kotlinx.coroutines.TimeoutCancellationException) {
                cancellation.cancel()
                client.failSafStorageRequest(
                    request.requestId,
                    SafStorageFailureKind.DEADLINE_EXCEEDED,
                    "SAF provider request exceeded its deadline",
                )
            } catch (error: SafStorageRequestException) {
                client.failSafStorageRequest(request.requestId, error.kind, error.message ?: "")
            } catch (error: SecurityException) {
                client.failSafStorageRequest(
                    request.requestId,
                    SafStorageFailureKind.GRANT_UNAVAILABLE,
                    error.message ?: "SAF grant is unavailable",
                )
            } catch (error: Throwable) {
                client.failSafStorageRequest(
                    request.requestId,
                    SafStorageFailureKind.PROVIDER_REFUSED,
                    error.message ?: error.toString(),
                )
            } finally {
                cancellation.cancel()
            }
        }
    }

    private fun traceUpdate(
        update: ViewUpdate,
        product: ProductState,
    ) {
        val kind =
            when (update.payload) {
                is ViewUpdatePayload.Snapshot -> "snapshot"
                is ViewUpdatePayload.Patch -> "patch"
                is ViewUpdatePayload.ResetRequired -> "reset"
            }
        val torrent =
            product.selectedTorrent?.let(product.torrents::get)
                ?: product.torrents.values.firstOrNull()
        val active = product.selectedTorrent?.let(product.pieces::get)?.active?.firstOrNull()
        val diagnostic = product.diagnostics.lastOrNull()
        val diagnosticDetail =
            diagnostic
                ?.fields
                ?.firstOrNull { it.key == "detail" }
                ?.value
                ?.let { it as? DiagnosticValue.Text }
                ?.value
                ?.replace(Regex("\\s+"), "_")
                ?: "none"
        Log.i(
            TAG,
            "view_update stream=${update.streamId} sequence=${update.sequence} " +
                "torrent=${torrent?.torrentId ?: "none"} " +
                "kind=$kind state=${torrent?.state?.name ?: "none"} " +
                "storage=${torrent?.storageState?.name ?: "none"} " +
                "metadata=${torrent?.metadataAvailable ?: false} " +
                "progress=${torrent?.progress?.disposition?.name ?: "none"} " +
                "reason=${torrent?.progress?.reason?.name ?: "none"} " +
                "diagnostic=${diagnostic?.code ?: "none"} " +
                "diagnostic_detail=$diagnosticDetail " +
                "verified=${torrent?.verifiedPieceCount ?: 0U} " +
                "piece=${active?.pieceIndex?.toString() ?: "none"} " +
                "requested=${active?.requested?.sumOf(::rangeBytes) ?: 0UL} " +
                "received=${active?.received?.sumOf(::rangeBytes) ?: 0UL} " +
                "stored=${active?.stored?.sumOf(::rangeBytes) ?: 0UL}",
        )
    }

    private fun rangeBytes(range: org.rstorrent.session.uniffi.IndexRange): ULong =
        (range.endExclusive - range.start).toULong()

    private fun observePowerAndNotification() {
        scope.launch {
            state.collect { product ->
                val active =
                    product.torrents.values.any {
                        it.state == TorrentState.AWAITING_METADATA ||
                            it.state == TorrentState.CHECKING ||
                            it.state == TorrentState.DOWNLOADING
                    }
                updatePowerLocks(active)
                val downloading =
                    product.torrents.values.count { it.state == TorrentState.DOWNLOADING }
                val detail =
                    when {
                        product.error != null -> "Error: ${product.error}"
                        downloading > 0 -> "Downloading $downloading torrent"
                        product.torrents.isEmpty() -> "Ready"
                        else -> "Transfers paused or complete"
                    }
                updateNotification(detail)
            }
        }
    }

    private fun updatePowerLocks(active: Boolean) {
        if (active) {
            if (powerLock == null) {
                val power = getSystemService(Context.POWER_SERVICE) as PowerManager
                powerLock =
                    power
                        .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "$packageName:download")
                        .apply {
                            setReferenceCounted(false)
                            acquire()
                        }
            }
            if (wifiLock == null) {
                val wifi =
                    applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
                wifiLock =
                    wifi
                        .createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "$packageName:download")
                        .apply {
                            setReferenceCounted(false)
                            acquire()
                        }
            }
        } else {
            releasePowerLocks()
        }
    }

    private fun releasePowerLocks() {
        powerLock?.let { if (it.isHeld) it.release() }
        wifiLock?.let { if (it.isHeld) it.release() }
        powerLock = null
        wifiLock = null
    }

    private suspend fun shutdown() {
        if (!stopped.compareAndSet(false, true)) return
        Log.i(TAG, "product_shutdown_begin")
        if (::presentationRepository.isInitialized) presentationRepository.close()
        trackerEvidenceJob?.cancel()
        trackerEvidenceSubscription?.close()
        if (::client.isInitialized) {
            try {
                Log.i(TAG, "product_shutdown_client_begin")
                client.shutdown()
                Log.i(TAG, "product_shutdown_client_complete")
            } finally {
                safStorageJobs.forEach { it.join() }
                client.close()
            }
        }
        releasePowerLocks()
        Log.i(TAG, "product_shutdown_complete")
    }

    private fun reportError(error: Throwable) {
        Log.e(TAG, "product control failed", error)
        mutableState.update { it.copy(error = error.message ?: error.toString()) }
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(
                NotificationChannel(
                    CHANNEL_ID,
                    "RSTorrent downloads",
                    NotificationManager.IMPORTANCE_LOW,
                ),
            )
        }
    }

    private fun notification(detail: String): Notification {
        val open =
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val stop =
            PendingIntent.getService(
                this,
                1,
                Intent(this, ProductEngineService::class.java).setAction(ACTION_STOP),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val builder =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                Notification.Builder(this, CHANNEL_ID)
            } else {
                @Suppress("DEPRECATION")
                Notification.Builder(this)
            }
        return builder
            .setSmallIcon(R.drawable.ic_rstorrent_notification)
            .setContentTitle("RSTorrent")
            .setContentText(detail)
            .setContentIntent(open)
            .setOngoing(true)
            .addAction(
                Notification.Action.Builder(
                    android.R.drawable.ic_menu_close_clear_cancel,
                    "Stop",
                    stop,
                ).build(),
            )
            .build()
    }

    private fun updateNotification(detail: String) {
        getSystemService(NotificationManager::class.java)
            .notify(NOTIFICATION_ID, notification(detail))
    }

    companion object {
        const val ACTION_STOP = "org.rstorrent.bootstrap.PRODUCT_STOP"
        private const val CHANNEL_ID = "rstorrent-product"
        private const val NOTIFICATION_ID = 42
        private const val MAX_TORRENT_SOURCE_BYTES = 64 * 1024 * 1024
        private const val SAF_PROVIDER_CONCURRENCY = 4
        private const val ANDROID_RATE_BYTES_PER_SECOND = 24 * 1024
        private const val TAG = "RSTorrentProduct"
    }
}
