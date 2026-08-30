package org.rstorrent.bootstrap

import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.IntentFilter
import android.database.Cursor
import android.net.Uri
import android.os.Binder
import android.os.Build
import android.os.CancellationSignal
import android.os.IBinder
import android.os.ParcelFileDescriptor
import android.os.PowerManager
import android.os.SystemClock
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.util.Log
import java.io.File
import java.io.FileNotFoundException
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.runInterruptible
import kotlinx.coroutines.supervisorScope
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.withTimeoutOrNull
import org.rstorrent.bootstrap.uniffi.AndroidApplicationClient
import org.rstorrent.bootstrap.uniffi.AndroidApplicationConfig
import org.rstorrent.bootstrap.uniffi.AndroidCompanionRootRequest
import org.rstorrent.bootstrap.uniffi.AndroidNetworkPolicy
import org.rstorrent.bootstrap.uniffi.AndroidPlatformStorageRoot
import org.rstorrent.bootstrap.uniffi.AndroidViewSubscription
import org.rstorrent.bootstrap.uniffi.SafStorageFailureKind
import org.rstorrent.bootstrap.uniffi.SafStorageOperation
import org.rstorrent.session.uniffi.Command
import org.rstorrent.session.uniffi.CommandResult
import org.rstorrent.session.uniffi.AddTorrentDisposition
import org.rstorrent.session.uniffi.AddTorrentResult
import org.rstorrent.session.uniffi.AddTorrentBytesRequest
import org.rstorrent.session.uniffi.CatalogPageRequest
import org.rstorrent.session.uniffi.ClientSettings
import org.rstorrent.session.uniffi.ClientSettingsPatch
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
import org.rstorrent.session.uniffi.StorageRootAvailability
import org.rstorrent.session.uniffi.SubscriptionSpec
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentSettingsPatch
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
    private val stopRequested = AtomicBoolean(false)
    private val latestStartId = AtomicInteger(0)
    @Volatile private var startCommandReceived = false
    private val shutdownComplete = CompletableDeferred<Unit>()
    private val clientReady = CompletableDeferred<Unit>()
    private val presentationReady = CompletableDeferred<Unit>()
    private val mutableState = MutableStateFlow(ProductState())
    val state: StateFlow<ProductState> = mutableState.asStateFlow()

    private lateinit var client: AndroidApplicationClient
    private lateinit var presentationRepository: AndroidPresentationRepository
    private lateinit var notificationCoordinator: AndroidNotificationCoordinator
    private var notificationBlockReceiver: BroadcastReceiver? = null
    private val notificationEligibilityMutation = Mutex()
    private val interactionLeases = ConcurrentHashMap.newKeySet<String>()
    private var trackerEvidenceSubscription: AndroidViewSubscription? = null
    private var trackerEvidenceJob: Job? = null
    private var companionPairingJob: Job? = null
    private var companionRootJob: Job? = null
    private var companionRootRemovalJob: Job? = null
    private val companionOwnerMutation = Mutex()
    @Volatile private var safStorageJobs: List<Job> = emptyList()
    @Volatile private var defaultSafRootId: String? = null
    private val safRootMutation = Mutex()
    private val safWork = ConcurrentHashMap.newKeySet<String>()
    private val safDirectCompletions = ConcurrentHashMap.newKeySet<String>()
    private val clientSettingsRequestActive = AtomicBoolean(false)
    private val torrentSettingsRequestActive = AtomicBoolean(false)
    private var powerLock: PowerManager.WakeLock? = null
    private val externalIntakeController = ExternalIntakeController()
    private val externalIntakeMutation = Mutex()
    private val externalContentMutation = Mutex()
    private val externalAdmissionHints = mutableMapOf<Long, ExternalContentHint>()
    private var externalAdmissionJob: Job? = null
    private var externalSubmissionJob: Job? = null
    @Volatile private var externalCancellationSignal: CancellationSignal? = null
    @Volatile private var externalAdmissionCancellationSignal: CancellationSignal? = null
    private var externalNoticeSequence = 0L

    private data class ExternalContentHint(
        val announcedMimeType: String?,
        val pathHasTorrentSuffix: Boolean,
    ) {
        val independentlyEligible: Boolean
            get() =
                announcedMimeType.equals(BITTORRENT_MIME_TYPE, ignoreCase = true) ||
                    pathHasTorrentSuffix
    }

    private data class ExternalContentMetadata(
        val displayLabel: String?,
        val knownLength: Long?,
        val providerMimeType: String?,
    )

    private sealed interface ExternalMetadataDisposition {
        data class Accepted(
            val displayLabel: String?,
            val knownLength: Long?,
        ) : ExternalMetadataDisposition

        data class Rejected(val reason: String) : ExternalMetadataDisposition
    }

    override fun onCreate() {
        super.onCreate()
        notificationCoordinator = AndroidNotificationCoordinator(this, mutableState)
        notificationCoordinator.initialize(interactionLeases.size)
        ProductInteractionRegistry.attach { leases ->
            interactionLeases.clear()
            interactionLeases.addAll(leases)
            if (startCommandReceived) refreshNotificationEligibility("interaction")
        }
        registerNotificationBlockReceiver()
        startForeground(
            AndroidNotificationContract.ONGOING_NOTIFICATION_ID,
            notificationCoordinator.ongoingNotification("Opening profile"),
        )
        val safRegistry = ProductSafRootRegistry.load(this)
        mutableState.update {
            it.copy(
                storageRootReady = false,
                storageRootLabel = safRegistry.roots.singleOrNull()?.label,
                preventSleepDuringActiveDownloads = ProductPowerPreference.read(this),
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
                            safRegistry.roots.map {
                                AndroidPlatformStorageRoot(it.rootId, it.label)
                            },
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
                            updateSafRootState(product)
                            traceUpdate(update, product)
                            if (driveSaf) advanceSaf(product)
                            driveSettingsMutations()
                        },
                        onTorrentListUpdate = notificationCoordinator::onTorrentListUpdate,
                        onTorrentListReset = notificationCoordinator::onTorrentListReset,
                        onError = ::reportError,
                    )
                presentationRepository.start(client)
                presentationReady.complete(Unit)
                safStorageJobs =
                    List(SAF_PROVIDER_CONCURRENCY) {
                        scope.launch(Dispatchers.IO) { driveSafStorageRequests() }
                    }
                reconcileSafRootRegistry()
                val storageRootHealthy = client.probeSafStorageRoots()
                Log.i(TAG, "saf_root_health source=startup available=$storageRootHealthy")
                refreshSafRootState()
                mutableState.update { it.copy(ready = true) }
                clientReady.complete(Unit)
                if (
                    ProductCompanionPreference.shouldStart(
                        isChromeOs(),
                        ProductCompanionPreference.read(this@ProductEngineService),
                    )
                ) {
                    startChromeOsCompanionOwners()
                }
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
                notificationCoordinator.updateOngoingNotification("RSTorrent needs attention")
            }
        }
    }

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        latestStartId.set(startId)
        startCommandReceived = true
        if (intent?.action == ACTION_STOP) {
            requestStop("notification_stop", startId)
        } else if (intent?.action == externalIntakeAction(packageName)) {
            admitExternalIntent(intent)
        } else if (intent?.action == ACTION_ENABLE_CHROMEOS_COMPANION) {
            enableChromeOsCompanion()
        }
        refreshNotificationEligibility("start")
        return if (stopRequested.get()) START_NOT_STICKY else START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        runBlocking(Dispatchers.IO) {
            shutdown("destroy")
        }
        scope.cancel()
        super.onDestroy()
    }

    override fun onTimeout(
        startId: Int,
        fgsType: Int,
    ) {
        Log.w(TAG, "product_service_timeout type=data_sync")
        requestStop("data_sync_timeout", startId)
    }

    private fun admitExternalIntent(command: Intent) {
        scope.launch {
            val started = SystemClock.elapsedRealtime()
            externalIntakeMutation.withLock {
                val forwardedRejection =
                    command.getStringExtra(MainActivity.EXTRA_EXTERNAL_REJECTION)
                if (forwardedRejection != null) {
                    val reason =
                        runCatching { ExternalIntentRejection.valueOf(forwardedRejection) }
                            .getOrNull()
                            ?.name
                            ?.lowercase()
                            ?: "invalid_forward"
                    rejectExternalIntent(reason, started)
                    return@withLock
                }
                val sourceValue = command.getStringExtra(MainActivity.EXTRA_EXTERNAL_SOURCE)
                val sourceUri = sourceValue?.let(Uri::parse)
                val classification =
                    ExternalIntentClassifier.classify(
                        ExternalIntentInput(
                            action = EXTERNAL_VIEW_ACTION,
                            data = sourceValue,
                            scheme = sourceUri?.scheme,
                            mimeType =
                                command.getStringExtra(MainActivity.EXTRA_EXTERNAL_MIME_TYPE),
                            path = sourceUri?.path,
                            hasSelector = false,
                            hasClipData = false,
                            packageOverride = null,
                            hasReadGrant =
                                command.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0,
                        ),
                    )
                when (classification) {
                    ExternalIntentClassification.NotExternalView ->
                        rejectExternalIntent("invalid_forward", started)
                    is ExternalIntentClassification.Rejected ->
                        rejectExternalIntent(classification.reason.name.lowercase(), started)
                    is ExternalIntentClassification.Magnet ->
                        admitExternalSource(
                            classification.source,
                            hint = null,
                            started = started,
                        )
                    is ExternalIntentClassification.Content ->
                        admitExternalSource(
                            classification.source,
                            ExternalContentHint(
                                classification.announcedMimeType,
                                classification.pathHasTorrentSuffix,
                            ),
                            started,
                        )
                }
            }
        }
    }

    private fun admitExternalSource(
        source: ExternalIntakeSource,
        hint: ExternalContentHint?,
        started: Long,
    ) {
        val result =
            externalIntakeController.receive(
                source,
                needsMetadataValidation = hint != null,
                rootReady = currentSafRootForAdd() != null,
            )
        val intakeId = result.intakeId
        when (result.disposition) {
            ExternalAdmissionDisposition.ADMITTED -> {
                if (hint != null && intakeId != null) externalAdmissionHints[intakeId] = hint
                logExternalIntake(
                    intakeId,
                    source.kind,
                    "received",
                    "accepted",
                    durationMillis = SystemClock.elapsedRealtime() - started,
                    disposition = "admitted",
                )
                publishExternalSnapshot()
                if (hint == null) {
                    logExternalIntake(intakeId, source.kind, "presented", "ready")
                } else {
                    scheduleExternalAdmissionLocked()
                }
            }
            ExternalAdmissionDisposition.COALESCED ->
                logExternalIntake(
                    intakeId,
                    source.kind,
                    "duplicate",
                    "exact_source",
                    durationMillis = SystemClock.elapsedRealtime() - started,
                    disposition = "coalesced",
                )
            ExternalAdmissionDisposition.QUEUE_FULL -> {
                publishExternalNotice(ExternalIntakeNoticeKind.QUEUE_FULL)
                logExternalIntake(
                    null,
                    source.kind,
                    "rejected",
                    "queue_full",
                    durationMillis = SystemClock.elapsedRealtime() - started,
                    disposition = "queue_full",
                )
            }
        }
    }

    private fun rejectExternalIntent(
        reason: String,
        started: Long,
    ) {
        publishExternalNotice(ExternalIntakeNoticeKind.REJECTED)
        logExternalIntake(
            null,
            null,
            "rejected",
            reason,
            durationMillis = SystemClock.elapsedRealtime() - started,
            disposition = "rejected",
        )
    }

    private fun scheduleExternalAdmissionLocked() {
        if (externalAdmissionJob?.isActive == true) return
        externalAdmissionJob =
            scope.launch {
                while (true) {
                    val pending =
                        externalIntakeMutation.withLock {
                            val work = externalIntakeController.nextReceived()
                            if (work == null) {
                                externalAdmissionJob = null
                                null
                            } else {
                                work to externalAdmissionHints.getValue(work.intakeId)
                            }
                        } ?: return@launch
                    val started = SystemClock.elapsedRealtime()
                    val disposition = inspectExternalContent(pending.first, pending.second)
                    externalIntakeMutation.withLock {
                        externalAdmissionHints.remove(pending.first.intakeId)
                        when (disposition) {
                            is ExternalMetadataDisposition.Accepted -> {
                                externalIntakeController.completeContentAdmission(
                                    pending.first.intakeId,
                                    accepted = true,
                                    displayLabel = disposition.displayLabel,
                                    knownLength = disposition.knownLength,
                                    rootReady = currentSafRootForAdd() != null,
                                )
                                logExternalIntake(
                                    pending.first.intakeId,
                                    ExternalIntakeKind.TORRENT_FILE,
                                    "presented",
                                    "metadata_accepted",
                                    durationMillis =
                                        SystemClock.elapsedRealtime() - started,
                                    disposition = "admitted",
                                )
                            }
                            is ExternalMetadataDisposition.Rejected -> {
                                externalIntakeController.completeContentAdmission(
                                    pending.first.intakeId,
                                    accepted = false,
                                    displayLabel = null,
                                    knownLength = null,
                                    rootReady = currentSafRootForAdd() != null,
                                )
                                publishExternalNotice(ExternalIntakeNoticeKind.REJECTED)
                                logExternalIntake(
                                    pending.first.intakeId,
                                    ExternalIntakeKind.TORRENT_FILE,
                                    "rejected",
                                    disposition.reason,
                                    durationMillis =
                                        SystemClock.elapsedRealtime() - started,
                                    disposition = "rejected",
                                )
                            }
                        }
                        publishExternalSnapshot()
                    }
                }
            }
    }

    private suspend fun inspectExternalContent(
        work: ExternalIntakeWork,
        hint: ExternalContentHint,
    ): ExternalMetadataDisposition =
        externalContentMutation.withLock {
            val metadata =
                try {
                    queryExternalContentMetadata(Uri.parse(work.source.reveal()))
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Throwable) {
                    return@withLock if (hint.independentlyEligible) {
                        ExternalMetadataDisposition.Accepted(null, null)
                    } else {
                        ExternalMetadataDisposition.Rejected("metadata_unavailable")
                    }
                }
            if (metadata.providerMimeType == DocumentsContract.Document.MIME_TYPE_DIR) {
                return@withLock ExternalMetadataDisposition.Rejected("directory")
            }
            val accepted =
                hint.independentlyEligible ||
                    metadata.providerMimeType.equals(
                        BITTORRENT_MIME_TYPE,
                        ignoreCase = true,
                    ) ||
                    hasTorrentSuffix(metadata.displayLabel)
            if (accepted) {
                ExternalMetadataDisposition.Accepted(
                    boundedExternalDisplayLabel(metadata.displayLabel),
                    metadata.knownLength,
                )
            } else {
                ExternalMetadataDisposition.Rejected("unsupported_content")
            }
        }

    private suspend fun queryExternalContentMetadata(uri: Uri): ExternalContentMetadata =
        supervisorScope {
            val cancellation = CancellationSignal()
            externalAdmissionCancellationSignal = cancellation
            val query =
                async(Dispatchers.IO) {
                    runInterruptible {
                        contentResolver
                            .query(
                                uri,
                                arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE),
                                null,
                                null,
                                null,
                                cancellation,
                            ).use(::externalContentMetadataFromCursor)
                    }
                }
            try {
                val result =
                    withTimeoutOrNull(EXTERNAL_PROVIDER_TIMEOUT_MILLIS) {
                        query.await()
                    }
                if (result == null) {
                    cancellation.cancel()
                    query.cancelAndJoin()
                    throw TorrentSourceReadTimeoutException()
                }
                result.copy(
                    providerMimeType = runInterruptible(Dispatchers.IO) {
                        contentResolver.getType(uri)
                    },
                )
            } finally {
                externalAdmissionCancellationSignal = null
                cancellation.cancel()
            }
        }

    private fun externalContentMetadataFromCursor(cursor: Cursor?): ExternalContentMetadata {
        if (cursor == null || !cursor.moveToFirst()) {
            return ExternalContentMetadata(null, null, null)
        }
        val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
        val sizeIndex = cursor.getColumnIndex(OpenableColumns.SIZE)
        return ExternalContentMetadata(
            displayLabel =
                nameIndex.takeIf { it >= 0 && !cursor.isNull(it) }?.let(cursor::getString),
            knownLength =
                sizeIndex.takeIf { it >= 0 && !cursor.isNull(it) }
                    ?.let(cursor::getLong)
                    ?.takeIf { it >= 0L },
            providerMimeType = null,
        )
    }

    fun setExternalIntakeStartContent(
        intakeId: Long,
        startContent: Boolean,
    ) {
        scope.launch {
            externalIntakeMutation.withLock {
                if (externalIntakeController.setStartContent(intakeId, startContent)) {
                    publishExternalSnapshot()
                }
            }
        }
    }

    fun confirmExternalIntake(intakeId: Long) {
        scope.launch {
            externalIntakeMutation.withLock {
                val work =
                    externalIntakeController.confirm(
                        intakeId,
                        rootReady = currentSafRootForAdd() != null,
                    )
                publishExternalSnapshot()
                if (work != null) scheduleExternalSubmissionLocked(work)
            }
        }
    }

    fun retryExternalIntake(intakeId: Long) {
        scope.launch {
            externalIntakeMutation.withLock {
                val work =
                    externalIntakeController.retry(
                        intakeId,
                        rootReady = currentSafRootForAdd() != null,
                    )
                publishExternalSnapshot()
                if (work != null) scheduleExternalSubmissionLocked(work)
            }
        }
    }

    fun cancelExternalIntake(intakeId: Long) {
        scope.launch {
            val submission =
                externalIntakeMutation.withLock {
                    if (!externalIntakeController.cancel(intakeId, currentSafRootForAdd() != null)) {
                        return@withLock null
                    }
                    externalAdmissionHints.remove(intakeId)
                    externalCancellationSignal?.cancel()
                    publishExternalSnapshot()
                    externalSubmissionJob
                }
            submission?.cancelAndJoin()
            logExternalIntake(intakeId, null, "cancelled", "user")
        }
    }

    private fun scheduleExternalSubmissionLocked(work: ExternalIntakeWork) {
        check(externalSubmissionJob?.isActive != true) {
            "external intake already has a submission job"
        }
        externalSubmissionJob =
            scope.launch {
                val started = SystemClock.elapsedRealtime()
                try {
                    clientReady.await()
                    val storageRoot = currentSafRootForAdd()
                    if (storageRoot == null) {
                        externalIntakeMutation.withLock {
                            externalIntakeController.submissionRootUnavailable(work.intakeId)
                            publishExternalSnapshot()
                        }
                        logExternalIntake(
                            work.intakeId,
                            work.source.kind,
                            "awaiting_root",
                            "root_unavailable",
                        )
                        return@launch
                    }
                    val result =
                        when (work.source.kind) {
                            ExternalIntakeKind.MAGNET ->
                                dispatchAddResult(
                                    Command.AddMagnet(
                                        work.source.reveal().trim(),
                                        storageRoot,
                                        work.startContent,
                                        emptyList(),
                                    ),
                                )
                            ExternalIntakeKind.TORRENT_FILE -> {
                                externalContentMutation.withLock {
                                    val source = readExternalTorrentSource(work)
                                    logExternalIntake(
                                        work.intakeId,
                                        work.source.kind,
                                        "source_read",
                                        "complete",
                                        byteCount = source.sourceBytes,
                                        peakSourceBytes = source.peakOwnedBytes,
                                    )
                                    dispatchTorrentSourceResult(
                                        source.bytes,
                                        work.startContent,
                                        FileSelectionIntent.All,
                                        storageRoot,
                                    )
                                }
                            }
                        }
                    val notice =
                        when (result.disposition) {
                            AddTorrentDisposition.Added -> ExternalIntakeNoticeKind.ADDED
                            AddTorrentDisposition.AlreadyPresent ->
                                ExternalIntakeNoticeKind.ALREADY_PRESENT
                            is AddTorrentDisposition.SelectionExpanded ->
                                ExternalIntakeNoticeKind.SELECTION_EXPANDED
                        }
                    externalIntakeMutation.withLock {
                        if (currentSafRootForAdd() == null) {
                            if (
                                externalIntakeController.submissionRootUnavailable(
                                    work.intakeId,
                                )
                            ) {
                                publishExternalSnapshot()
                                logExternalIntake(
                                    work.intakeId,
                                    work.source.kind,
                                    "awaiting_root",
                                    "root_unavailable",
                                    durationMillis =
                                        SystemClock.elapsedRealtime() - started,
                                )
                            }
                            return@withLock
                        }
                        if (
                            externalIntakeController.completeSubmission(
                                work.intakeId,
                                currentSafRootForAdd() != null,
                            )
                        ) {
                            publishExternalNotice(notice)
                            publishExternalSnapshot()
                            logExternalIntake(
                                work.intakeId,
                                work.source.kind,
                                when (notice) {
                                    ExternalIntakeNoticeKind.ALREADY_PRESENT -> "duplicate"
                                    else -> "success"
                                },
                                "complete",
                                durationMillis = SystemClock.elapsedRealtime() - started,
                                disposition = notice.name.lowercase(),
                            )
                        }
                    }
                } catch (error: CancellationException) {
                    throw error
                } catch (error: Throwable) {
                    val retryable =
                        error is SecurityException ||
                            error is FileNotFoundException ||
                            error is TorrentSourceReadTimeoutException ||
                            error is TorrentSourceProviderException
                    val reason =
                        when (error) {
                            is SecurityException -> "permission"
                            is FileNotFoundException -> "provider_missing"
                            is TorrentSourceReadTimeoutException -> "timeout"
                            is TorrentSourceProviderException -> "provider_failure"
                            is EmptyTorrentSourceException -> "empty"
                            is OversizedTorrentSourceException -> "oversized"
                            else -> "invalid_or_engine_failure"
                        }
                    externalIntakeMutation.withLock {
                        val failureDisposition =
                            externalIntakeController.failSubmission(
                                work.intakeId,
                                retryable,
                                currentSafRootForAdd() != null,
                            )
                        if (failureDisposition != null) {
                            if (
                                failureDisposition ==
                                ExternalSubmissionFailureDisposition.TERMINAL
                            ) {
                                publishExternalNotice(
                                    ExternalIntakeNoticeKind.TERMINAL_FAILURE,
                                )
                            }
                            publishExternalSnapshot()
                            logExternalIntake(
                                work.intakeId,
                                work.source.kind,
                                when (failureDisposition) {
                                    ExternalSubmissionFailureDisposition.RETRYABLE -> "retry"
                                    ExternalSubmissionFailureDisposition.TERMINAL -> "terminal"
                                },
                                reason,
                                durationMillis = SystemClock.elapsedRealtime() - started,
                                disposition =
                                    when (failureDisposition) {
                                        ExternalSubmissionFailureDisposition.RETRYABLE -> "retryable"
                                        ExternalSubmissionFailureDisposition.TERMINAL -> "failed"
                                    },
                            )
                        }
                    }
                } finally {
                    externalCancellationSignal = null
                    externalIntakeMutation.withLock {
                        externalSubmissionJob = null
                    }
                }
            }
    }

    private suspend fun readExternalTorrentSource(work: ExternalIntakeWork): BoundedTorrentSource {
        val cancellation = CancellationSignal()
        externalCancellationSignal = cancellation
        val uri = Uri.parse(work.source.reveal())
        return BoundedTorrentSourceReader.read(
            openInput = {
                val descriptor =
                    contentResolver.openFileDescriptor(uri, "r", cancellation)
                        ?: throw FileNotFoundException()
                ParcelFileDescriptor.AutoCloseInputStream(descriptor)
            },
            knownLength = work.knownLength,
            cancelled = { cancellation.isCanceled },
            onCancel = cancellation::cancel,
        )
    }

    private fun publishExternalSnapshot() {
        val snapshot = externalIntakeController.snapshot()
        mutableState.update {
            it.copy(
                externalIntake = snapshot.presentation,
                externalIntakeDepth = snapshot.descriptorCount,
            )
        }
    }

    private fun publishExternalNotice(kind: ExternalIntakeNoticeKind) {
        externalNoticeSequence += 1
        mutableState.update {
            it.copy(
                externalIntakeNotice = ExternalIntakeNotice(externalNoticeSequence, kind),
            )
        }
    }

    private fun logExternalIntake(
        intakeId: Long?,
        kind: ExternalIntakeKind?,
        phase: String,
        reason: String,
        durationMillis: Long = 0L,
        disposition: String = "none",
        byteCount: Int = 0,
        peakSourceBytes: Int = 0,
    ) {
        Log.i(
            TAG,
            "external_intake id=${intakeId ?: 0L} " +
                "kind=${kind?.name?.lowercase() ?: "unknown"} phase=$phase " +
                "reason=$reason count=1 depth=${externalIntakeController.descriptorCount()} " +
                "duration_ms=$durationMillis disposition=$disposition " +
                "bytes=$byteCount peak_source_bytes=$peakSourceBytes",
        )
    }

    fun addMagnet(
        magnet: String,
        skipFiles: List<UInt> = emptyList(),
        startContent: Boolean = true,
    ) {
        val storageRoot = currentSafRootForAdd()
        if (storageRoot == null) {
            mutableState.update { it.copy(error = "Select a download folder first") }
            return
        }
        scope.launch {
            try {
                clientReady.await()
                dispatchAddAwait(
                    Command.AddMagnet(magnet.trim(), storageRoot, startContent, skipFiles),
                    magnetV1(magnet),
                    magnetV2(magnet),
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
        val storageRoot = currentSafRootForAdd()
        if (storageRoot == null) {
            mutableState.update { it.copy(error = "Select a download folder first") }
            return
        }
        scope.launch(Dispatchers.IO) {
            try {
                clientReady.await()
                val source = readTorrentSource(uri)
                dispatchTorrentSource(source, startContent, storageRoot = storageRoot)
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun addTorrentBytes(
        source: ByteArray,
        startContent: Boolean = true,
        selection: FileSelectionIntent = FileSelectionIntent.All,
    ) {
        val storageRoot = currentSafRootForAdd()
        if (storageRoot == null) {
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
                dispatchTorrentSource(source, startContent, selection, storageRoot)
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    private suspend fun dispatchTorrentSource(
        source: ByteArray,
        startContent: Boolean,
        selection: FileSelectionIntent = FileSelectionIntent.All,
        storageRoot: String = requireNotNull(currentSafRootForAdd()) {
            "Select a download folder first"
        },
    ) {
        val add = dispatchTorrentSourceResult(source, startContent, selection, storageRoot)
        logAddResult(add, null)
    }

    private suspend fun dispatchTorrentSourceResult(
        source: ByteArray,
        startContent: Boolean,
        selection: FileSelectionIntent,
        storageRoot: String,
    ): AddTorrentResult {
        val request =
            AddTorrentBytesRequest(
                version = 1U.toUShort(),
                requestId = "android-$requestPrefix-${requestIds.getAndIncrement()}",
                expectedRevision = null,
                storageRoot = storageRoot,
                startContent = startContent,
                selection = selection,
                sourceLength = source.size.toUInt(),
            )
        val response = client.addTorrentBytes(request, source)
        val outcome = response.outcome
        if (outcome is ResponseOutcome.Error) error(outcome.error.message)
        return addResult(response)
    }

    private suspend fun readTorrentSource(uri: Uri): ByteArray =
        BoundedTorrentSourceReader.read(
            openInput = {
                contentResolver.openInputStream(uri) ?: throw FileNotFoundException()
            },
        ).bytes

    fun addMagnetWithTrackerPolicyForTest(
        magnet: String,
        policyName: String,
        startContent: Boolean,
    ) {
        check(ProductSafDocuments.isDebuggable(this)) {
            "tracker authentication injection is debug-only"
        }
        val storageRoot = currentSafRootForAdd()
        if (storageRoot == null) {
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
                    Command.UpdateClientSettings(
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
                        ).asPatch(),
                    ),
                )
                awaitTrackerPolicy(policy)
                val torrentId = dispatchAddAwait(
                    Command.AddMagnet(magnet.trim(), storageRoot, startContent, emptyList()),
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
        val storageRoot = currentSafRootForAdd()
        if (storageRoot == null) {
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
                    Command.UpdateClientSettings(
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
                        ).asPatch(),
                    ),
                )
                awaitEncryptionPolicy(policy)
                dispatchAddAwait(
                    Command.AddMagnet(magnet.trim(), storageRoot, true, skipFiles),
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
                        Command.UpdateClientSettings(
                            clientSettingsPatch(ipv6Enabled = false),
                        ),
                    )
                    logIpv6Evidence("disabled", awaitIpv6Policy(false))
                    return@launch
                }
                if (mode == "enable_sequence") {
                    logIpv6Evidence("restarted", current)
                    dispatchAwait(
                        Command.UpdateClientSettings(
                            clientSettingsPatch(ipv6Enabled = true),
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
                            Command.UpdateClientSettings(
                                clientSettingsPatch(ipv6Enabled = desired),
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
                        Command.UpdateClientSettings(
                            clientSettingsPatch(
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

    fun setSafTree(
        treeUri: Uri,
        repairRootId: String? = null,
        companionRequestId: String? = null,
    ) {
        ProductSafRootRegistry.recordSelectionCandidate(this, treeUri, repairRootId)
        scope.launch {
            try {
                clientReady.await()
                reconcileSafRootRegistry()
                val storageRootHealthy = client.probeSafStorageRoots()
                refreshSafRootState()
                Log.i(TAG, "saf_root_health source=selection available=$storageRootHealthy")
                Log.i(TAG, "saf_tree_ready root=${defaultSafRootId ?: "none"}")
                if (companionRequestId != null) {
                    completeCompanionRootSelection(companionRequestId, repairRootId)
                }
                if (!storageRootHealthy) return@launch
                advanceSaf(mutableState.value)
                mutableState.value.torrents.values
                    .filter { it.state == TorrentState.AWAITING_STORAGE }
                    .forEach { resume(it.torrentId) }
            } catch (error: Throwable) {
                if (companionRequestId != null && ::client.isInitialized) {
                    client.failCompanionRootRequest(
                        companionRequestId,
                        error.message ?: "Unable to use the selected download folder",
                    )
                    cancelCompanionRootNotification()
                }
                reportError(error)
            }
        }
    }

    fun cancelCompanionRootRequest(requestId: String) {
        scope.launch {
            try {
                clientReady.await()
                client.completeCompanionRootRequest(requestId, null)
                cancelCompanionRootNotification()
            } catch (error: Throwable) {
                if (!stopped.get()) reportError(error)
            }
        }
    }

    fun approveCompanionPairing(requestId: String) {
        resolveCompanionPairing(requestId, approve = true)
    }

    fun rejectCompanionPairing(requestId: String) {
        resolveCompanionPairing(requestId, approve = false)
    }

    private fun resolveCompanionPairing(requestId: String, approve: Boolean) {
        scope.launch {
            try {
                clientReady.await()
                if (approve) client.approveCompanionPairing(requestId)
                else client.rejectCompanionPairing(requestId)
                mutableState.update {
                    if (it.companionPairing?.requestId == requestId) {
                        it.copy(companionPairing = null)
                    } else {
                        it
                    }
                }
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    private fun enableChromeOsCompanion() {
        if (!isChromeOs()) {
            Log.w(TAG, "ignored ChromeOS companion enable request on a non-ChromeOS device")
            return
        }
        ProductCompanionPreference.enable(this)
        scope.launch {
            try {
                clientReady.await()
                startChromeOsCompanionOwners()
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    private suspend fun startChromeOsCompanionOwners() {
        companionOwnerMutation.withLock {
            check(isChromeOs()) { "ChromeOS companion listener is unavailable on this device" }
            val port = client.startChromeosCompanion()
            mutableState.update { it.copy(companionPort = port) }
            if (companionPairingJob?.isActive != true) {
                companionPairingJob =
                    scope.launch {
                        while (!stopped.get()) {
                            val pending = client.pendingCompanionPairing()
                            mutableState.update {
                                it.copy(
                                    companionPairing =
                                        pending?.let { request ->
                                            CompanionPairingState(
                                                request.requestId,
                                                request.extensionId,
                                                request.extensionName,
                                                request.installationId,
                                                request.expiresInSeconds,
                                            )
                                        },
                                )
                            }
                            delay(COMPANION_PAIRING_POLL_MILLIS)
                        }
                    }
            }
            if (companionRootJob?.isActive != true) {
                companionRootJob =
                    scope.launch {
                        while (!stopped.get()) {
                            val request = client.nextCompanionRootRequest() ?: return@launch
                            presentCompanionRootRequest(request)
                        }
                    }
            }
            if (companionRootRemovalJob?.isActive != true) {
                companionRootRemovalJob =
                    scope.launch {
                        while (!stopped.get()) {
                            val request =
                                client.nextCompanionRootRemovalRequest() ?: return@launch
                            executeCompanionRootRemoval(request)
                        }
                    }
            }
            Log.i(TAG, "chromeos_companion_ready port=$port")
        }
    }

    private suspend fun completeCompanionRootSelection(
        requestId: String,
        repairRootId: String?,
    ) {
        val storage = client.safStorageSnapshot()
        val selectedRootId = repairRootId ?: storage.defaultRoot
        val root = storage.roots.singleOrNull { it.rootId == selectedRootId }
        if (root?.availability != StorageRootAvailability.AVAILABLE) {
            client.failCompanionRootRequest(
                requestId,
                "The selected download folder is unavailable",
            )
        } else {
            client.completeCompanionRootRequest(requestId, root)
        }
        cancelCompanionRootNotification()
    }

    private fun presentCompanionRootRequest(request: AndroidCompanionRootRequest) {
        val open =
            Intent(this, MainActivity::class.java).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                putExtra(MainActivity.EXTRA_COMPANION_ROOT_REQUEST, request.requestId)
                request.repairRoot?.let {
                    putExtra(MainActivity.EXTRA_COMPANION_REPAIR_ROOT, it)
                }
            }
        val pending =
            PendingIntent.getActivity(
                this,
                COMPANION_ROOT_PENDING_INTENT_ID,
                open,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val posted = notificationCoordinator.showCompanionRootNotification(pending)
        runCatching { startActivity(open) }
            .onFailure {
                if (posted) {
                    Log.i(TAG, "companion root picker waits for notification action")
                } else {
                    Log.i(TAG, "companion root picker requires visible Android interaction")
                }
            }
    }

    private fun cancelCompanionRootNotification() {
        notificationCoordinator.cancelCompanionRootNotification()
    }

    private fun isChromeOs(): Boolean =
        packageManager.hasSystemFeature("org.chromium.arc") ||
            packageManager.hasSystemFeature("org.chromium.arc.device_management")

    fun makeSafRootCurrent(rootId: String) {
        scope.launch {
            try {
                clientReady.await()
                safRootMutation.withLock {
                    val storage = client.safStorageSnapshot()
                    val root = requireNotNull(storage.roots.singleOrNull { it.rootId == rootId }) {
                        "Download folder is not registered"
                    }
                    require(root.availability == StorageRootAvailability.AVAILABLE) {
                        "Repair this download folder before making it current"
                    }
                    if (storage.defaultRoot == rootId) return@withLock
                    executeSafRootOperation(
                        ProductSafRootRegistry.beginSetDefault(
                            this@ProductEngineService,
                            rootId,
                        ),
                    )
                    refreshSafRootState()
                }
                mutableState.update { it.copy(error = null) }
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    fun removeSafRoot(rootId: String) {
        scope.launch {
            try {
                clientReady.await()
                safRootMutation.withLock {
                    val storage = client.safStorageSnapshot()
                    require(storage.defaultRoot != rootId) {
                        "The current download folder cannot be removed"
                    }
                    require(storage.roots.any { it.rootId == rootId }) {
                        "Download folder is not registered"
                    }
                    require(mutableState.value.torrents.values.none { it.storageRoot == rootId }) {
                        "Download folder is still used by a retained torrent"
                    }
                    executeSafRootOperation(
                        ProductSafRootRegistry.beginRemoval(
                            this@ProductEngineService,
                            rootId,
                        ),
                    )
                    refreshSafRootState()
                }
                mutableState.update { it.copy(error = null) }
            } catch (error: Throwable) {
                reportError(error)
            }
        }
    }

    private suspend fun reconcileSafRootRegistry() {
        safRootMutation.withLock {
            var state = ProductSafRootRegistry.load(this@ProductEngineService)
            if (state.pending != null) executeSafRootOperation(state.pending)
            state = ProductSafRootRegistry.load(this@ProductEngineService)
            if (state.selectionCandidate != null) {
                val candidate = Uri.parse(state.selectionCandidate)
                val label = ProductSafDocuments.treeLabel(this@ProductEngineService, candidate)
                val operation =
                    state.selectionRepairRootId?.let { rootId ->
                        ProductSafRootRegistry.beginRepair(
                            this@ProductEngineService,
                            rootId,
                            candidate,
                            label,
                        )
                    } ?: ProductSafRootRegistry.beginSelection(
                        this@ProductEngineService,
                        client.allocateSafStorageRootId(),
                        label,
                    )
                executeSafRootOperation(operation)
            }
            refreshSafRootState()
        }
    }

    private suspend fun executeCompanionRootRemoval(
        request: org.rstorrent.bootstrap.uniffi.AndroidCompanionRootRemovalRequest,
    ) {
        val command = request.applicationRequest.command as? Command.RemoveStorageRoot
        if (command == null) {
            client.failCompanionRootRemovalRequest(
                request.requestId,
                "Companion root-removal request has the wrong command",
            )
            return
        }
        var nativeMutationComplete = false
        try {
            safRootMutation.withLock {
                val operation =
                    ProductSafRootRegistry.beginRemoval(
                        this@ProductEngineService,
                        command.storageRoot,
                    )
                val response = client.dispatch(request.applicationRequest)
                if (response.outcome is ResponseOutcome.Error) {
                    ProductSafRootRegistry.abandonPendingRemoval(this@ProductEngineService)
                } else {
                    nativeMutationComplete = true
                    ProductSafRootRegistry.completePending(this@ProductEngineService)
                    operation.previous?.let { previous ->
                        runCatching {
                            ProductSafDocuments.releaseGrantIfUnregistered(
                                this@ProductEngineService,
                                Uri.parse(previous.treeUri),
                            )
                        }.onFailure { releaseError ->
                            Log.w(TAG, "could not release removed SAF grant", releaseError)
                        }
                    }
                    refreshSafRootState()
                }
                client.completeCompanionRootRemovalRequest(request.requestId, response)
            }
        } catch (error: Throwable) {
            runCatching {
                if (
                    !nativeMutationComplete &&
                    ProductSafRootRegistry.load(this@ProductEngineService)
                        .pending?.kind == ProductSafRootOperationKind.REMOVE
                ) {
                    ProductSafRootRegistry.abandonPendingRemoval(this@ProductEngineService)
                }
            }.onFailure(error::addSuppressed)
            client.failCompanionRootRemovalRequest(
                request.requestId,
                error.message ?: "Android root removal failed",
            )
            reportError(error)
        }
    }

    private suspend fun executeSafRootOperation(operation: ProductSafRootOperation) {
        var nativeMutationComplete = false
        try {
            when (operation.kind) {
                ProductSafRootOperationKind.ADD,
                ProductSafRootOperationKind.SET_DEFAULT,
                -> client.installSafStorageRoot(
                    operation.rootId,
                    operation.label,
                    operation.makeDefault,
                )
                ProductSafRootOperationKind.REPAIR -> {
                    val mutation =
                        client.repairSafStorageRoot(operation.rootId, operation.label)
                    mutation.restartTorrentIds.forEach(::resume)
                }
                ProductSafRootOperationKind.REMOVE -> {
                    if (
                        client.safStorageSnapshot().roots.any {
                            it.rootId == operation.rootId
                        }
                    ) {
                        dispatchAwait(Command.RemoveStorageRoot(operation.rootId))
                    }
                }
            }
            nativeMutationComplete = true
            ProductSafRootRegistry.completePending(this)
            operation.previous?.let { previous ->
                runCatching {
                    ProductSafDocuments.releaseGrantIfUnregistered(
                        this,
                        Uri.parse(previous.treeUri),
                    )
                }.onFailure { releaseError ->
                    Log.w(TAG, "could not release replaced SAF grant", releaseError)
                }
            }
            Log.i(
                TAG,
                "saf_root_operation kind=${operation.kind.name.lowercase()} " +
                    "root=${operation.rootId} result=complete",
            )
        } catch (error: Throwable) {
            if (nativeMutationComplete) throw error
            when (operation.kind) {
                ProductSafRootOperationKind.ADD -> {
                    ProductSafRootRegistry.rollbackAdd(this)
                    releaseFailedSafSelection(operation)
                }
                ProductSafRootOperationKind.REPAIR -> {
                    ProductSafRootRegistry.rollbackRepair(this)
                    releaseFailedSafSelection(operation)
                    client.probeSafStorageRoots()
                    mutableState.value.torrents.values
                        .filter { it.storageRoot == operation.rootId }
                        .filter { it.state == TorrentState.AWAITING_STORAGE }
                        .forEach { resume(it.torrentId) }
                }
                ProductSafRootOperationKind.SET_DEFAULT ->
                    ProductSafRootRegistry.abandonPendingDefault(this)
                ProductSafRootOperationKind.REMOVE ->
                    ProductSafRootRegistry.abandonPendingRemoval(this)
            }
            throw error
        }
    }

    private fun releaseFailedSafSelection(operation: ProductSafRootOperation) {
        runCatching {
            ProductSafDocuments.releaseGrantIfUnregistered(this, Uri.parse(operation.treeUri))
        }.onFailure { releaseError ->
            Log.w(TAG, "could not release rejected SAF grant", releaseError)
        }
    }

    private suspend fun refreshSafRootState() {
        val storage = client.safStorageSnapshot()
        defaultSafRootId = storage.defaultRoot
        val current = storage.roots.singleOrNull { it.rootId == storage.defaultRoot }
        val ready =
            current?.availability == StorageRootAvailability.AVAILABLE &&
                ProductSafRootRegistry.treeForRoot(this, current.rootId) != null
        mutableState.update {
            it.copy(
                storage = storage,
                storageRootReady = ready,
                storageRootLabel = current?.label,
                error =
                    when {
                        current == null -> null
                        ready -> it.error?.takeUnless { message ->
                            message == "Selected download folder is unavailable"
                        }
                        else -> "Selected download folder is unavailable"
                    },
            )
        }
        notifyExternalRootAvailability(ready)
    }

    private fun updateSafRootState(product: ProductState) {
        val storage = product.storage ?: return
        defaultSafRootId = storage.defaultRoot
        val current = storage.roots.singleOrNull { it.rootId == storage.defaultRoot }
        val ready =
            current?.availability == StorageRootAvailability.AVAILABLE &&
                ProductSafRootRegistry.treeForRoot(this, current.rootId) != null
        mutableState.update {
            it.copy(
                storageRootReady = ready,
                storageRootLabel = current?.label,
            )
        }
        notifyExternalRootAvailability(ready)
    }

    private fun notifyExternalRootAvailability(ready: Boolean) {
        scope.launch {
            externalIntakeMutation.withLock {
                externalIntakeController.rootAvailabilityChanged(ready)
                publishExternalSnapshot()
            }
        }
    }

    private fun currentSafRootForAdd(): String? {
        val rootId = defaultSafRootId ?: return null
        val root = mutableState.value.storage?.roots?.singleOrNull { it.rootId == rootId }
        return rootId.takeIf {
            root?.availability == StorageRootAvailability.AVAILABLE &&
                ProductSafRootRegistry.treeForRoot(this, rootId) != null
        }
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
                when {
                    action.startsWith("download_file:") -> {
                        val fileIndex =
                            action.substringAfter(':').toUIntOrNull()
                                ?: error("download-file action has an invalid index")
                        dispatchAwait(Command.DownloadFiles(torrentId, listOf(fileIndex)))
                    }
                    action == "pause" -> dispatchAwait(Command.Pause(torrentId))
                    action == "resume" -> dispatchAwait(Command.Resume(torrentId))
                    action == "force_recheck" -> forceRecheckAndAwaitForTest(torrentId)
                    action == "remove" ->
                        dispatchAwait(
                            Command.RemoveTorrent(
                                torrentId,
                                RemovalDataPolicy.DELETE_DATA,
                            ),
                        )
                    action == "enable_upload" -> {
                        val current = awaitIpv6Policy(null)
                        dispatchAwait(
                            Command.UpdateClientSettings(
                                clientSettingsPatch(
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
                var productState = ProductState()
                while (true) {
                    val update = subscription.nextUpdate() ?: error("torrent view closed")
                    val torrent =
                        when (update.payload) {
                            is ViewUpdatePayload.ResetRequired -> {
                                subscription.resync()
                                productState = ProductState()
                                null
                            }
                            else -> {
                                productState = ProductStateReducer.reduce(productState, update)
                                productState.torrents[torrentId]
                            }
                        }
                    when (torrent?.state) {
                        TorrentState.CHECKING -> {
                            sawChecking = true
                            torrent.checking?.let { progress ->
                                Log.i(
                                    TAG,
                                    "force_recheck_progress torrent=$torrentId " +
                                        "processed=${progress.piecesProcessed} " +
                                        "matched=${progress.piecesMatched} " +
                                        "absent=${progress.piecesAbsent} " +
                                        "mismatched=${progress.piecesMismatched}",
                                )
                            }
                        }
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

    fun setFilePriority(
        torrentId: String,
        fileIndex: UInt,
        priority: FilePriority,
    ) {
        dispatch(
            Command.SetFilePriority(
                torrentId,
                listOf(fileIndex),
                priority,
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
        storageRoot: String,
        torrentName: String,
        file: FileView,
    ) {
        scope.launch(Dispatchers.IO) {
            try {
                require(file.verifiedBytes == file.lengthBytes) { "File is not complete" }
                val tree =
                    ProductSafRootRegistry.treeForRoot(this@ProductEngineService, storageRoot)
                        ?: error("Download folder is unavailable")
                val path =
                    if (file.path.firstOrNull() == torrentName) {
                        file.path
                    } else {
                        listOf(torrentName) + file.path
                    }
                val document =
                    ProductSafDocuments.contentDocument(this@ProductEngineService, tree, path)
                        ?: error("Completed file is unavailable")
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

    fun updateClientSettings(patch: ClientSettingsPatch) {
        val changes = patch.fieldValues()
        if (changes.isEmpty()) {
            mutableState.update { it.copy(error = "Settings update is empty") }
            return
        }
        mutableState.update { product ->
            val configured = product.clientSettings?.configured
                ?: return@update product.copy(error = "Settings are still loading")
            val initialized =
                if (product.clientSettingsDraft.resourceKey == null) {
                    product.clientSettingsDraft.authority(
                        "client-settings",
                        product.latestDurableRevision(),
                        configured.fieldValues(),
                    )
                } else {
                    product.clientSettingsDraft
                }
            product.copy(clientSettingsDraft = initialized.edit(changes), error = null)
        }
        driveClientSettingsMutation()
    }

    fun setPreventSleepDuringActiveDownloads(enabled: Boolean) {
        if (!ProductPowerPreference.persist(this, enabled)) {
            mutableState.update { it.copy(error = "Power setting could not be saved") }
            return
        }
        mutableState.update {
            it.copy(
                preventSleepDuringActiveDownloads = enabled,
                error = null,
            )
        }
        Log.i(TAG, "prevent_sleep_setting enabled=$enabled")
    }

    fun updateTorrentSettings(
        torrentId: String,
        patch: TorrentSettingsPatch,
    ) {
        val changes = patch.fieldValues()
        if (changes.isEmpty()) {
            mutableState.update { it.copy(error = "Torrent settings update is empty") }
            return
        }
        mutableState.update { product ->
            val torrent = product.torrents[torrentId]
                ?: return@update product.copy(error = "Torrent is no longer present")
            val initialized =
                if (product.torrentSettingsDraft.resourceKey != torrentId) {
                    product.torrentSettingsDraft.authority(
                        torrentId,
                        product.latestDurableRevision(),
                        torrent.transferLimits.fieldValues(),
                    )
                } else {
                    product.torrentSettingsDraft
                }
            product.copy(torrentSettingsDraft = initialized.edit(changes), error = null)
        }
        driveTorrentSettingsMutation()
    }

    private fun driveSettingsMutations() {
        driveClientSettingsMutation()
        driveTorrentSettingsMutation()
    }

    private fun driveClientSettingsMutation() {
        if (!clientSettingsRequestActive.compareAndSet(false, true)) return
        var request: SettingsDraftRequest<ClientSettingsField>? = null
        mutableState.update { product ->
            val captured = captureSettingsDraftRequest(product.clientSettingsDraft)
            request = captured.request
            product.copy(clientSettingsDraft = captured.draft)
        }
        val outbound = request
        if (outbound == null) {
            clientSettingsRequestActive.set(false)
            if (mutableState.value.clientSettingsDraft.hasDispatchableSettingsDraft()) {
                driveClientSettingsMutation()
            }
            return
        }
        scope.launch {
            try {
                clientReady.await()
                val response =
                    dispatchForResponse(
                        Command.UpdateClientSettings(outbound.values.toClientSettingsPatch()),
                        outbound.expectedRevision,
                    )
                mutableState.update {
                    it.copy(
                        clientSettingsDraft =
                            it.clientSettingsDraft.accepted(
                                outbound.resourceKey,
                                response.revision,
                            ),
                    )
                }
            } catch (error: Throwable) {
                mutableState.update {
                    it.copy(
                        clientSettingsDraft =
                            it.clientSettingsDraft.failed(
                                outbound.resourceKey,
                                error.message ?: error.toString(),
                            ),
                    )
                }
                reportError(error)
            } finally {
                clientSettingsRequestActive.set(false)
                driveClientSettingsMutation()
            }
        }
    }

    private fun driveTorrentSettingsMutation() {
        if (!torrentSettingsRequestActive.compareAndSet(false, true)) return
        var request: SettingsDraftRequest<TorrentSettingsField>? = null
        mutableState.update { product ->
            val captured = captureSettingsDraftRequest(product.torrentSettingsDraft)
            request = captured.request
            product.copy(torrentSettingsDraft = captured.draft)
        }
        val outbound = request
        if (outbound == null) {
            torrentSettingsRequestActive.set(false)
            if (mutableState.value.torrentSettingsDraft.hasDispatchableSettingsDraft()) {
                driveTorrentSettingsMutation()
            }
            return
        }
        scope.launch {
            try {
                clientReady.await()
                val response =
                    dispatchForResponse(
                        Command.UpdateTorrentSettings(
                            outbound.resourceKey,
                            outbound.values.toTorrentSettingsPatch(),
                        ),
                        outbound.expectedRevision,
                    )
                mutableState.update {
                    it.copy(
                        torrentSettingsDraft =
                            it.torrentSettingsDraft.accepted(
                                outbound.resourceKey,
                                response.revision,
                            ),
                    )
                }
            } catch (error: Throwable) {
                mutableState.update {
                    it.copy(
                        torrentSettingsDraft =
                            it.torrentSettingsDraft.failed(
                                outbound.resourceKey,
                                error.message ?: error.toString(),
                            ),
                    )
                }
                reportError(error)
            } finally {
                torrentSettingsRequestActive.set(false)
                driveTorrentSettingsMutation()
            }
        }
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
        requestStop("ui_stop")
    }

    fun refreshNotificationEligibility() {
        refreshNotificationEligibility("activity_result")
    }

    internal fun setNotificationPreference(
        preference: ProductNotificationPreference,
        enabled: Boolean,
    ) {
        notificationCoordinator.setPreference(preference, enabled, interactionLeases.size)
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

    private fun magnetV1(magnet: String): String? =
        Regex("(?i)urn:btih:([0-9a-f]{40})")
            .find(magnet)
            ?.groupValues
            ?.get(1)
            ?.lowercase()

    private fun magnetV2(magnet: String): String? =
        Regex("(?i)urn:btmh:1220([0-9a-f]{64})")
            .find(magnet)
            ?.groupValues
            ?.get(1)
            ?.lowercase()

    private suspend fun dispatchAddAwait(
        command: Command,
        v1InfoHash: String?,
        v2InfoHash: String? = null,
    ): String = logAddResult(dispatchAddResult(command), v1InfoHash, v2InfoHash)

    private suspend fun dispatchAddResult(command: Command): AddTorrentResult =
        addResult(dispatchForResponse(command))

    private fun addResult(
        response: org.rstorrent.session.uniffi.ResponseEnvelope,
    ): AddTorrentResult =
        (response.result as? CommandResult.AddTorrent)?.result
            ?: error("add response omitted its result")

    private fun logAddResult(
        add: AddTorrentResult,
        v1InfoHash: String?,
        v2InfoHash: String? = null,
    ): String {
        Log.i(
            TAG,
            "torrent_added torrent=${add.torrentId} " +
                "protocol_v1=${v1InfoHash ?: "unknown"} " +
                "protocol_v2=${v2InfoHash ?: "unknown"} " +
                "disposition=${add.disposition::class.simpleName}",
        )
        return add.torrentId
    }

    private suspend fun dispatchForResponse(
        command: Command,
        expectedRevision: String? = null,
    ): org.rstorrent.session.uniffi.ResponseEnvelope {
        val response =
            client.dispatch(
                RequestEnvelope(
                    1U.toUShort(),
                    "android-$requestPrefix-${requestIds.getAndIncrement()}",
                    expectedRevision,
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
        safDirectCompletions.retainAll(product.torrents.keys)
        for (torrent in product.torrents.values) {
            if (torrent.state == TorrentState.COMPLETE && torrent.removalState == null) {
                if (safDirectCompletions.add(torrent.torrentId)) {
                    scope.launch {
                        try {
                            val snapshot = client.safStoragePoolSnapshot()
                            Log.i(
                                TAG,
                                "saf_direct_complete torrent=${torrent.torrentId} " +
                                    "limit=${snapshot.limit} " +
                                    "owned_high_water=${snapshot.ownedHighWater} " +
                                    "pending_high_water=${snapshot.platformPendingHighWater}",
                            )
                        } catch (error: Throwable) {
                            safDirectCompletions.remove(torrent.torrentId)
                            reportError(error)
                        }
                    }
                }
            }
            if (torrent.removalState != RemovalState.AWAITING_PLATFORM) continue
            val action = "removal"
            val key = "${torrent.torrentId}:$action"
            if (!safWork.add(key)) continue
            scope.launch {
                try {
                    when (action) {
                        "removal" -> {
                            Log.i(TAG, "saf_removal_begin torrent=${torrent.torrentId}")
                            val plan = client.safRemovalPlan(torrent.torrentId)
                            val treeUri =
                                ProductSafRootRegistry.treeForRoot(
                                    this@ProductEngineService,
                                    plan.storageRoot,
                                ) ?: throw SafStorageRequestException(
                                    SafStorageFailureKind.GRANT_UNAVAILABLE,
                                    "persisted SAF grant is unavailable for the torrent root",
                                )
                            ProductSafDocuments.deleteData(
                                this@ProductEngineService,
                                treeUri,
                                plan,
                            )
                            client.confirmSafRemoval(torrent.torrentId, plan.operationId)
                            safDirectCompletions.remove(torrent.torrentId)
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
                        ProductSafRootRegistry.treeForRoot(
                            this@ProductEngineService,
                            request.rootId,
                        )
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
                "v1=${torrent?.protocolIdentities?.v1 ?: "none"} " +
                "v2=${torrent?.protocolIdentities?.v2 ?: "none"} " +
                "kind=$kind state=${torrent?.state?.name ?: "none"} " +
                "storage=${torrent?.storageState?.name ?: "none"} " +
                "metadata=${torrent?.metadataAvailable ?: false} " +
                "progress=${torrent?.progress?.disposition?.name ?: "none"} " +
                "reason=${torrent?.progress?.reason?.name ?: "none"} " +
                "diagnostic=${diagnostic?.code ?: "none"} " +
                "diagnostic_detail=$diagnosticDetail " +
                "verified=${torrent?.verifiedPieceCount ?: 0U} " +
                "check=${torrent?.checking?.let { progress ->
                    "${progress.piecesMatched},${progress.piecesAbsent},${progress.piecesMismatched}"
                } ?: "none"} " +
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
                    product.preventSleepDuringActiveDownloads &&
                        requiresSleepInhibition(product.torrents.values)
                updatePowerLock(active)
                notificationCoordinator.updateOngoingNotification(
                    productOngoingNotificationText(product),
                )
            }
        }
    }

    private fun updatePowerLock(active: Boolean) {
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
                Log.i(TAG, "partial_wake_lock acquired=true")
            }
        } else {
            releasePowerLock()
        }
    }

    private fun releasePowerLock() {
        powerLock?.let {
            if (it.isHeld) {
                it.release()
                Log.i(TAG, "partial_wake_lock acquired=false")
            }
        }
        powerLock = null
    }

    private fun requestStop(
        reason: String,
        startId: Int? = null,
    ) {
        if (!stopRequested.compareAndSet(false, true)) return
        scope.launch {
            try {
                shutdown(reason)
            } finally {
                stopForeground(STOP_FOREGROUND_REMOVE)
                val safeStartId = maxOf(startId ?: 0, latestStartId.get())
                if (safeStartId == 0 || !stopSelfResult(safeStartId)) stopSelf()
            }
        }
    }

    private suspend fun shutdown(reason: String) {
        if (!stopped.compareAndSet(false, true)) {
            shutdownComplete.await()
            return
        }
        Log.i(TAG, "product_shutdown_begin reason=$reason")
        try {
            unregisterNotificationBlockReceiver()
            ProductInteractionRegistry.detach()
            interactionLeases.clear()
            externalAdmissionCancellationSignal?.cancel()
            externalCancellationSignal?.cancel()
            externalAdmissionJob?.cancelAndJoin()
            externalSubmissionJob?.cancelAndJoin()
            if (::presentationRepository.isInitialized) presentationRepository.close()
            trackerEvidenceJob?.cancel()
            trackerEvidenceSubscription?.close()
            trackerEvidenceJob?.join()
            companionPairingJob?.cancel()
            companionRootJob?.cancel()
            companionRootRemovalJob?.cancel()
            notificationCoordinator.close()
            if (::client.isInitialized) {
                try {
                    Log.i(TAG, "product_shutdown_client_begin")
                    client.shutdown()
                    Log.i(TAG, "product_shutdown_client_complete")
                } finally {
                    companionPairingJob?.join()
                    companionRootJob?.join()
                    companionRootRemovalJob?.join()
                    safStorageJobs.forEach { it.join() }
                    client.close()
                }
            }
            Log.i(TAG, "product_shutdown_complete reason=$reason")
        } finally {
            releasePowerLock()
            shutdownComplete.complete(Unit)
        }
    }

    private fun reportError(error: Throwable) {
        Log.e(TAG, "product control failed", error)
        mutableState.update { it.copy(error = error.message ?: error.toString()) }
    }

    private fun refreshNotificationEligibility(reason: String) {
        if (stopped.get()) return
        scope.launch {
            notificationEligibilityMutation.withLock {
                if (stopped.get()) return@withLock
                val eligibility =
                    notificationCoordinator.refreshPlatformState(interactionLeases.size)
                Log.i(
                    TAG,
                    "notification_eligibility reason=$reason visible=" +
                        "${eligibility.backgroundNotificationVisible} " +
                        "interaction=${eligibility.interactionLeaseCount > 0}",
                )
                if (eligibility.shouldStopOwner) {
                    requestStop("notification_visibility")
                }
            }
        }
    }

    private fun registerNotificationBlockReceiver() {
        val receiver =
            object : BroadcastReceiver() {
                override fun onReceive(
                    context: Context?,
                    intent: Intent?,
                ) {
                    refreshNotificationEligibility("block_state")
                }
            }
        val filter =
            IntentFilter().apply {
                addAction(NotificationManager.ACTION_APP_BLOCK_STATE_CHANGED)
                addAction(NotificationManager.ACTION_NOTIFICATION_CHANNEL_BLOCK_STATE_CHANGED)
            }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("DEPRECATION")
            registerReceiver(receiver, filter)
        }
        notificationBlockReceiver = receiver
    }

    private fun unregisterNotificationBlockReceiver() {
        notificationBlockReceiver?.let { receiver ->
            runCatching { unregisterReceiver(receiver) }
        }
        notificationBlockReceiver = null
    }

    companion object {
        const val ACTION_STOP = "org.rstorrent.bootstrap.PRODUCT_STOP"
        const val ACTION_ENABLE_CHROMEOS_COMPANION =
            "org.rstorrent.bootstrap.ENABLE_CHROMEOS_COMPANION"
        fun externalIntakeAction(packageName: String): String =
            "$packageName.action.EXTERNAL_TORRENT_INTAKE"
        private const val COMPANION_ROOT_PENDING_INTENT_ID = 43
        const val INTERACTION_ACTIVITY = "activity"
        const val INTERACTION_NOTIFICATION_PERMISSION = "notification_permission"
        const val INTERACTION_NOTIFICATION_SETTINGS = "notification_settings"
        const val INTERACTION_SAF_PICKER = "saf_picker"
        const val INTERACTION_TORRENT_PICKER = "torrent_picker"
        private const val COMPANION_PAIRING_POLL_MILLIS = 250L
        private const val EXTERNAL_PROVIDER_TIMEOUT_MILLIS = 30_000L
        private const val SAF_PROVIDER_CONCURRENCY = 4
        private const val ANDROID_RATE_BYTES_PER_SECOND = 24 * 1024
        private const val TAG = "RSTorrentProduct"
    }
}
