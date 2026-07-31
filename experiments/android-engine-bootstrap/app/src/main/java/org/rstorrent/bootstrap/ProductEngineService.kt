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
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CompletableDeferred
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
import org.rstorrent.bootstrap.uniffi.AndroidApplicationClient
import org.rstorrent.bootstrap.uniffi.AndroidApplicationConfig
import org.rstorrent.bootstrap.uniffi.AndroidNetworkPolicy
import org.rstorrent.bootstrap.uniffi.AndroidViewSubscription
import org.rstorrent.session.uniffi.Command
import org.rstorrent.session.uniffi.DeliveryPolicy
import org.rstorrent.session.uniffi.DiagnosticCategory
import org.rstorrent.session.uniffi.DiagnosticFilter
import org.rstorrent.session.uniffi.DiagnosticProfile
import org.rstorrent.session.uniffi.DiagnosticSeverity
import org.rstorrent.session.uniffi.RequestEnvelope
import org.rstorrent.session.uniffi.ResponseOutcome
import org.rstorrent.session.uniffi.SubscriptionSpec
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.ViewProjection
import org.rstorrent.session.uniffi.ViewSelector
import org.rstorrent.session.uniffi.ViewUpdate
import org.rstorrent.session.uniffi.ViewUpdatePayload

class ProductEngineService : Service() {
    inner class LocalBinder : Binder() {
        val service: ProductEngineService
            get() = this@ProductEngineService
    }

    private val binder = LocalBinder()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
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
    private var diagnosticTorrentOnly = false
    private var diagnosticProfile = DiagnosticProfile.NORMAL
    private var diagnosticSeverity = DiagnosticSeverity.INFO
    private var diagnosticCategories: List<DiagnosticCategory> = emptyList()
    private var selectedTorrent: String? = null
    private var safTreeUri: Uri? = null
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
                            (32 * 1024).toULong(),
                        ),
                    )
                listSubscription =
                    client.subscribe(
                        SubscriptionSpec(
                            ViewSelector.TorrentList,
                            ViewProjection.SUMMARY,
                            DeliveryPolicy(250U, 256U * 1024U),
                            null,
                        ),
                    )
                listJob = consume(requireNotNull(listSubscription), driveSaf = true)
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
        dispatch(Command.AddMagnet(magnet.trim(), "downloads", emptyList()))
    }

    fun setSafTree(treeUri: Uri) {
        safTreeUri = treeUri
        mutableState.update {
            it.copy(
                storageRootReady = true,
                storageRootLabel = treeUri.lastPathSegment,
                error = null,
            )
        }
        advanceSaf(mutableState.value)
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
                val response =
                    client.dispatch(
                        RequestEnvelope(
                            1U.toUShort(),
                            "android-${requestIds.getAndIncrement()}",
                            null,
                            command,
                        ),
                    )
                val outcome = response.outcome
                if (outcome is ResponseOutcome.Error) {
                    error(outcome.error.message)
                }
            } catch (error: Throwable) {
                reportError(error)
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
                when (torrent.state) {
                    TorrentState.AWAITING_STORAGE -> "storage"
                    TorrentState.AWAITING_PUBLICATION -> "publication"
                    else -> continue
                }
            val key = "${torrent.torrentId}:$action"
            if (!safWork.add(key)) continue
            scope.launch {
                try {
                    when (action) {
                        "storage" -> {
                            Log.i(TAG, "saf_storage_open torrent=${torrent.torrentId}")
                            val plan = client.safStoragePlan(torrent.torrentId)
                            Log.i(TAG, "saf_storage_planned torrent=${torrent.torrentId}")
                            ProductSafDocuments.openStaging(this@ProductEngineService, treeUri, plan)
                                .use { handles ->
                                    Log.i(
                                        TAG,
                                        "saf_storage_descriptors_open torrent=${torrent.torrentId}",
                                    )
                                    client.startSaf(torrent.torrentId, handles.storage())
                                }
                            Log.i(TAG, "saf_storage_started torrent=${torrent.torrentId}")
                        }
                        "publication" -> {
                            Log.i(TAG, "saf_publication_begin torrent=${torrent.torrentId}")
                            check(client.preparedSafFiles(torrent.torrentId).isNotEmpty()) {
                                "native prepared publication manifest is empty"
                            }
                            val plan = client.safStoragePlan(torrent.torrentId)
                            ProductSafDocuments
                                .publishAndOpen(this@ProductEngineService, treeUri, plan)
                                .use { handles ->
                                    if (crashAfterSafRename.compareAndSet(true, false)) {
                                        Log.i(
                                            TAG,
                                            "saf_test_crash_after_rename " +
                                                "torrent=${torrent.torrentId}",
                                        )
                                        android.os.Process.killProcess(android.os.Process.myPid())
                                        error("process survived SAF publication crash injection")
                                    }
                                    client.confirmSafPublication(
                                        torrent.torrentId,
                                        handles.descriptors(),
                                    )
                                }
                            Log.i(TAG, "saf_publication_confirmed torrent=${torrent.torrentId}")
                        }
                        else -> error("unknown SAF action $action")
                    }
                } catch (error: Throwable) {
                    if (action == "storage") {
                        try {
                            client.markSafUnavailable(
                                torrent.torrentId,
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
        val active = selectedTorrent?.let(product.pieces::get)?.active
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
        listSubscription?.close()
        pieceSubscription?.close()
        diagnosticSubscription?.close()
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
        private const val TAG = "RSTorrentProduct"
    }
}
