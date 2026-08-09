package org.rstorrent.bootstrap

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Binder
import android.os.Build
import android.os.CancellationSignal
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
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
import org.rstorrent.session.uniffi.Command
import org.rstorrent.session.uniffi.CatalogPageRequest
import org.rstorrent.session.uniffi.ClientSettings
import org.rstorrent.session.uniffi.ClientSettingsApplicationState
import org.rstorrent.session.uniffi.ClientSettingsRuntimeView
import org.rstorrent.session.uniffi.DeliveryPolicy
import org.rstorrent.session.uniffi.DiagnosticCategory
import org.rstorrent.session.uniffi.DiagnosticFilter
import org.rstorrent.session.uniffi.DiagnosticProfile
import org.rstorrent.session.uniffi.DiagnosticSeverity
import org.rstorrent.session.uniffi.EncryptionPolicy
import org.rstorrent.session.uniffi.HttpsServerAuthenticationPolicy
import org.rstorrent.session.uniffi.ListenerPolicy
import org.rstorrent.session.uniffi.PortMappingPolicy
import org.rstorrent.session.uniffi.RequestEnvelope
import org.rstorrent.session.uniffi.RemovalState
import org.rstorrent.session.uniffi.ResponseOutcome
import org.rstorrent.session.uniffi.SubscriptionSpec
import org.rstorrent.session.uniffi.TorrentState
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
    private val mutableState = MutableStateFlow(ProductState())
    val state: StateFlow<ProductState> = mutableState.asStateFlow()

    private lateinit var client: AndroidApplicationClient
    private var listSubscription: AndroidViewSubscription? = null
    private var listJob: Job? = null
    private var pieceSubscription: AndroidViewSubscription? = null
    private var pieceJob: Job? = null
    private var diagnosticSubscription: AndroidViewSubscription? = null
    private var diagnosticJob: Job? = null
    private var trackerEvidenceSubscription: AndroidViewSubscription? = null
    private var trackerEvidenceJob: Job? = null
    private var diagnosticTorrentOnly = false
    private var diagnosticProfile = DiagnosticProfile.NORMAL
    private var diagnosticSeverity = DiagnosticSeverity.INFO
    private var diagnosticCategories: List<DiagnosticCategory> = emptyList()
    private var selectedTorrent: String? = null
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
                storageRootReady = safTreeUri != null,
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
                listSubscription =
                    client.subscribe(
                        SubscriptionSpec(
                            ViewSelector.TorrentList,
                            ViewProjection.SUMMARY,
                            DeliveryPolicy(250U, 256U * 1024U),
                            null,
                            null,
                        ),
                    )
                listJob = consume(requireNotNull(listSubscription), driveSaf = true)
                repeat(SAF_PROVIDER_CONCURRENCY) {
                    scope.launch(Dispatchers.IO) { driveSafStorageRequests() }
                }
                subscribeDiagnostics()
                mutableState.update { it.copy(ready = true, error = null) }
                clientReady.complete(Unit)
                observePowerAndNotification()
            } catch (error: Throwable) {
                if (!clientReady.isCompleted) {
                    clientReady.completeExceptionally(error)
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

    fun addMagnet(magnet: String) {
        if (safTreeUri == null) {
            mutableState.update { it.copy(error = "Select a download folder first") }
            return
        }
        dispatch(Command.AddMagnet(magnet.trim(), "downloads", true, emptyList()))
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
        val torrentId =
            Regex("(?i)urn:btih:([0-9a-f]{40})")
                .find(magnet)
                ?.groupValues
                ?.get(1)
                ?.lowercase()
                ?: error("test magnet has no hexadecimal v1 identity")
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
                            encryption = EncryptionPolicy.ALLOW,
                            ipv6Enabled = true,
                            trackerHttpsServerAuthentication = policy,
                        ),
                    ),
                )
                awaitTrackerPolicy(policy)
                dispatchAwait(
                    Command.AddMagnet(magnet.trim(), "downloads", startContent, emptyList()),
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
                            encryption = policy,
                            ipv6Enabled = true,
                            trackerHttpsServerAuthentication =
                                HttpsServerAuthenticationPolicy.SYSTEM_TRUST,
                        ),
                    ),
                )
                awaitEncryptionPolicy(policy)
                dispatchAwait(
                    Command.AddMagnet(magnet.trim(), "downloads", true, emptyList()),
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
                Log.i(TAG, "saf_tree_ready uri=$treeUri")
                mutableState.update {
                    it.copy(
                        storageRootReady = true,
                        storageRootLabel = treeUri.lastPathSegment,
                        error = null,
                    )
                }
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

    fun pause(torrentId: String) {
        dispatch(Command.Pause(torrentId))
    }

    fun resume(torrentId: String) {
        dispatch(Command.Resume(torrentId))
    }

    fun selectTorrent(torrentId: String) {
        if (selectedTorrent == torrentId) return
        selectedTorrent = torrentId
        mutableState.update { it.copy(selectedTorrent = torrentId) }
        if (diagnosticTorrentOnly) subscribeDiagnostics()
        pieceJob?.cancel()
        pieceSubscription?.close()
        pieceJob = null
        pieceSubscription = null
        scope.launch {
            try {
                clientReady.await()
                val subscription =
                    client.subscribe(
                        SubscriptionSpec(
                            ViewSelector.Torrent(torrentId),
                            ViewProjection.PIECE_ACTIVITY,
                            DeliveryPolicy(0U, 256U * 1024U),
                            null,
                            null,
                        ),
                    )
                if (selectedTorrent != torrentId) {
                    subscription.close()
                    return@launch
                }
                pieceSubscription = subscription
                pieceJob = consume(subscription, driveSaf = false)
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun configureDiagnostics(
        profile: DiagnosticProfile,
        severity: DiagnosticSeverity,
        categories: List<DiagnosticCategory>,
        torrentOnly: Boolean,
    ) {
        diagnosticProfile = profile
        diagnosticSeverity = severity
        diagnosticCategories = categories
        diagnosticTorrentOnly = torrentOnly
        subscribeDiagnostics()
    }

    private fun subscribeDiagnostics() {
        diagnosticJob?.cancel()
        diagnosticSubscription?.close()
        diagnosticJob = null
        diagnosticSubscription = null
        scope.launch {
            try {
                clientReady.await()
                val selector =
                    if (diagnosticTorrentOnly && selectedTorrent != null) {
                        ViewSelector.Torrent(requireNotNull(selectedTorrent))
                    } else {
                        ViewSelector.TorrentList
                    }
                val subscription =
                    client.subscribe(
                        SubscriptionSpec(
                            selector,
                            ViewProjection.DIAGNOSTICS,
                            DeliveryPolicy(100U, 256U * 1024U),
                            DiagnosticFilter(
                                diagnosticProfile,
                                diagnosticSeverity,
                                diagnosticCategories,
                            ),
                            null,
                        ),
                    )
                diagnosticSubscription = subscription
                diagnosticJob = consume(subscription, driveSaf = false)
            } catch (error: Throwable) {
                if (!stopped.get()) reportError(error)
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

    private fun consume(
        subscription: AndroidViewSubscription,
        driveSaf: Boolean,
    ): Job =
        scope.launch {
            try {
                while (true) {
                    val update = subscription.nextUpdate() ?: break
                    try {
                        var reduced: ProductState? = null
                        mutableState.update { current ->
                            ProductStateReducer.reduce(current, update).also {
                                reduced = it
                            }
                        }
                        val product = requireNotNull(reduced)
                        traceUpdate(update, product)
                        if (driveSaf) advanceSaf(product)
                        if (selectedTorrent == null) {
                            product.torrents.keys.firstOrNull()?.let(::selectTorrent)
                        }
                    } catch (_: ViewResetRequiredException) {
                        mutableState.update {
                            it.copy(diagnosticResets = it.diagnosticResets + 1UL)
                        }
                        subscription.resync()
                    }
                }
            } catch (error: Throwable) {
                if (!stopped.get()) reportError(error)
            } finally {
                subscription.close()
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
                            ProductSafDocuments.publish(this@ProductEngineService, treeUri, name)
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
                    if (request.delete) {
                        ProductSafDocuments.deleteDynamic(
                            this@ProductEngineService,
                            treeUri,
                            request,
                        )
                        client.completeSafStorageDelete(request.requestId)
                    } else {
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
            selectedTorrent?.let(product.torrents::get)
                ?: product.torrents.values.firstOrNull()
        val active = selectedTorrent?.let(product.pieces::get)?.active?.firstOrNull()
        Log.i(
            TAG,
            "view_update stream=${update.streamId} sequence=${update.sequence} " +
                "kind=$kind state=${torrent?.state?.name ?: "none"} " +
                "storage=${torrent?.storageState?.name ?: "none"} " +
                "metadata=${torrent?.metadataAvailable ?: false} " +
                "progress=${torrent?.progress?.disposition?.name ?: "none"} " +
                "reason=${torrent?.progress?.reason?.name ?: "none"} " +
                "diagnostic=${product.diagnostics.lastOrNull()?.code ?: "none"} " +
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
        listJob?.cancel()
        pieceJob?.cancel()
        diagnosticJob?.cancel()
        trackerEvidenceJob?.cancel()
        listSubscription?.close()
        pieceSubscription?.close()
        diagnosticSubscription?.close()
        trackerEvidenceSubscription?.close()
        if (::client.isInitialized) {
            try {
                client.shutdown()
            } finally {
                client.close()
            }
        }
        releasePowerLocks()
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
            .setSmallIcon(android.R.drawable.stat_sys_download)
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
        private const val SAF_PROVIDER_CONCURRENCY = 4
        private const val TAG = "RSTorrentProduct"
    }
}
