package org.rstorrent.bootstrap

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Debug
import android.os.IBinder
import android.os.SystemClock
import android.util.Base64
import android.util.Log
import java.io.File
import java.io.FileOutputStream
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONArray
import org.json.JSONObject
import org.rstorrent.bootstrap.uniffi.EngineConfig
import org.rstorrent.bootstrap.uniffi.EngineReport
import org.rstorrent.bootstrap.uniffi.EngineSession
import org.rstorrent.bootstrap.uniffi.JoinResult
import org.rstorrent.bootstrap.uniffi.SessionSnapshot
import org.rstorrent.bootstrap.uniffi.StartDisposition
import org.rstorrent.bootstrap.uniffi.StartResult
import org.rstorrent.bootstrap.uniffi.TerminalResult
import org.rstorrent.bootstrap.uniffi.TerminalOutcome
import org.rstorrent.bootstrap.uniffi.interfaceVersion

class EngineService : Service() {
    private data class ActiveRun(
        val runId: String,
        val scenario: String,
        val root: File,
        val resultPath: File,
        val eventsPath: File,
        val config: EngineConfig,
        val startResult: StartResult,
        val startedElapsed: Long,
        val fdCountBefore: Int,
        val saf: DirectSafRun? = null,
        val completed: AtomicBoolean = AtomicBoolean(false),
    )

    private val commandExecutor =
        Executors.newSingleThreadExecutor { task ->
            Thread(task, "rstorrent-android-commands")
        }
    private val waitExecutor =
        Executors.newCachedThreadPool { task ->
            Thread(task, "rstorrent-android-wait")
        }
    private lateinit var session: EngineSession
    private val activeLock = Any()
    private var active: ActiveRun? = null
    private var nativeReady = false

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, notification("Starting engine"))
        try {
            PlatformTrustBootstrap.ensureInitialized(applicationContext)
            val actual = interfaceVersion()
            check(actual == BootstrapContract.EXPECTED_INTERFACE) {
                "native interface $actual does not match " +
                    BootstrapContract.EXPECTED_INTERFACE
            }
            session = EngineSession()
            nativeReady = true
            logEvent(
                JSONObject()
                    .put("event", "service_created")
                    .put("interface_version", actual),
            )
        } catch (error: Throwable) {
            Log.e(TAG, "native bootstrap failed", error)
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        if (!nativeReady || intent == null) {
            return START_NOT_STICKY
        }
        when (intent.action ?: BootstrapContract.ACTION_START) {
            BootstrapContract.ACTION_START ->
                commandExecutor.execute { startRun(intent) }
            BootstrapContract.ACTION_CANCEL ->
                commandExecutor.execute { cancelRun(intent) }
            BootstrapContract.ACTION_OBSERVE ->
                commandExecutor.execute { observeRun(intent) }
            BootstrapContract.ACTION_VERIFY ->
                commandExecutor.execute { verifyCompletedRun(intent) }
            else ->
                Log.w(TAG, "ignoring unknown action ${intent.action}")
        }
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        if (nativeReady) {
            val snapshot = session.snapshot()
            if (snapshot.taskAlive) {
                val ownedSession = session
                Thread(
                    {
                        val joined = ownedSession.cancelAndJoin(10_000UL)
                        Log.i(TAG, "shutdown_joined=${joined.joined}")
                        ownedSession.close()
                    },
                    "rstorrent-android-shutdown",
                ).start()
            } else {
                session.close()
            }
        }
        commandExecutor.shutdown()
        waitExecutor.shutdown()
        super.onDestroy()
    }

    private fun startRun(intent: Intent) {
        try {
            val duplicate = synchronized(activeLock) { active }
            if (duplicate != null) {
                val result = session.start(duplicate.config)
                appendEvent(
                    duplicate,
                    JSONObject()
                        .put("event", "duplicate_start")
                        .put("disposition", result.disposition.name)
                        .put("generation", result.generation.toLong()),
                )
                check(result.disposition == StartDisposition.BUSY) {
                    "active duplicate start was ${result.disposition}"
                }
                return
            }

            val run = prepareRun(intent) ?: return
            val startResult =
                run.saf?.start(session, run.config) ?: session.start(run.config)
            val activeRun = run.copy(
                startResult = startResult,
                startedElapsed = SystemClock.elapsedRealtime(),
            )
            appendEvent(
                activeRun,
                JSONObject()
                    .put("event", "engine_start")
                    .put("disposition", startResult.disposition.name)
                    .put("generation", startResult.generation.toLong()),
            )
            if (startResult.disposition != StartDisposition.STARTED) {
                run.saf?.let { SafDocuments.cleanup(this, it) }
                writeRejected(
                    activeRun.resultPath,
                    activeRun.runId,
                    activeRun.scenario,
                    "engine start ${startResult.disposition}: ${startResult.message}",
                )
                stopAfterTerminal()
                return
            }
            synchronized(activeLock) {
                check(active == null) { "active run changed during start" }
                active = activeRun
            }
            updateNotification("Downloading ${activeRun.runId}")
            waitExecutor.execute {
                val joined = session.waitForTerminal(180_000UL)
                completeRun(activeRun, joined, "terminal_wait")
            }
        } catch (error: Throwable) {
            Log.e(TAG, "start command failed", error)
            val runId = intent.getStringExtra("run_id")
            if (runId != null) {
                writeRejectedIfPossible(runId, "start command failed: $error")
            }
            stopAfterTerminal()
        }
    }

    private fun prepareRun(intent: Intent): ActiveRun? {
        val runId = BootstrapContract.requireRunId(intent.getStringExtra("run_id"))
        val scenario = intent.getStringExtra("scenario") ?: "success"
        val sessionsRoot = File(filesDir, "sessions")
        val resultsRoot = File(filesDir, "results")
        check(sessionsRoot.mkdirs() || sessionsRoot.isDirectory)
        check(resultsRoot.mkdirs() || resultsRoot.isDirectory)
        val root = File(sessionsRoot, runId)
        val resultPath = File(resultsRoot, "$runId.json")
        if (root.exists() || resultPath.exists()) {
            writeRejectedIfPossible(runId, "run paths already exist")
            stopAfterTerminal()
            return null
        }
        check(root.mkdir()) { "could not create exact session root" }
        val eventsPath = File(root, "events.jsonl")
        check(eventsPath.createNewFile()) { "events path already exists" }

        val metainfoBase64 = intent.getStringExtra("metainfo_base64")
            ?: error("metainfo_base64 is required")
        require(metainfoBase64.length <= BootstrapContract.MAX_METAINFO_BASE64_CHARS) {
            "metainfo_base64 exceeds the command limit"
        }
        val metainfo = Base64.decode(metainfoBase64, Base64.NO_WRAP)
        val metainfoPath = File(root, "fixture.torrent")
        check(metainfoPath.createNewFile()) { "metainfo path already exists" }
        FileOutputStream(metainfoPath).use { output ->
            output.write(metainfo)
            output.fd.sync()
        }

        val outputPath = File(root, "downloaded")
        val collision = intent.getStringExtra("collision") ?: ""
        val storage = intent.getStringExtra("storage") ?: "private"
        if (!storage.startsWith("saf-")) {
            createCollision(collision, root, outputPath, resultPath)
        }
        if (collision == "result") {
            logEvent(
                JSONObject()
                    .put("event", "preexisting_result_refused")
                    .put("run_id", runId),
            )
            stopAfterTerminal()
            return null
        }

        val skipFiles = parseIndexes(intent.getStringExtra("skip_files"))
        val saf =
            if (storage.startsWith("saf-")) {
                val treeUri =
                    Uri.parse(
                        requireNotNull(intent.getStringExtra("tree_uri")) {
                            "tree_uri is required for SAF storage"
                        },
                    )
                SafDocuments.prepare(
                    this,
                    treeUri,
                    metainfo,
                    skipFiles,
                )
            } else {
                null
            }
        val peerPort = intent.getIntExtra("peer_port", 0)
        require(peerPort in 1..65_535) { "peer_port is invalid" }
        val config = EngineConfig(
            metainfoPath.absolutePath,
            outputPath.absolutePath,
            peerPort.toUShort(),
            intent.getLongExtra("timeout_seconds", 30).toULong(),
            intent.getLongExtra(
                "max_buffered_payload_bytes",
                32L * 1024L,
            ).toULong(),
            intent.getLongExtra("storage_write_delay_millis", 0).toULong(),
            skipFiles,
        )
        return ActiveRun(
            runId = runId,
            scenario = scenario,
            root = root,
            resultPath = resultPath,
            eventsPath = eventsPath,
            config = config,
            startResult = StartResult(
                StartDisposition.REJECTED,
                0UL,
                "not started",
            ),
            startedElapsed = 0,
            fdCountBefore = fdCount(),
            saf = saf,
        )
    }

    private fun cancelRun(intent: Intent) {
        val run = synchronized(activeLock) { active }
        if (run == null) {
            logEvent(
                JSONObject()
                    .put("event", "cancel_without_active_run")
                    .put("run_id", intent.getStringExtra("run_id")),
            )
            stopAfterTerminal()
            return
        }
        val requestedRunId = intent.getStringExtra("run_id")
        if (requestedRunId != null && requestedRunId != run.runId) {
            appendEvent(
                run,
                JSONObject()
                    .put("event", "cancel_wrong_run")
                    .put("requested_run_id", requestedRunId),
            )
            return
        }
        val before = session.snapshot()
        appendEvent(
            run,
            JSONObject()
                .put("event", "cancel_requested")
                .put("state", before.state.name)
                .put("requested_bytes", before.requestedBytes.toLong())
                .put("received_bytes", before.receivedBytes.toLong())
                .put("stored_bytes", before.storedBytes.toLong()),
        )
        val began = SystemClock.elapsedRealtime()
        val joined = session.cancelAndJoin(10_000UL)
        appendEvent(
            run,
            JSONObject()
                .put("event", "cancel_join")
                .put("joined", joined.joined)
                .put(
                    "elapsed_millis",
                    SystemClock.elapsedRealtime() - began,
                ),
        )
        completeRun(run, joined, "explicit_cancel")
    }

    private fun observeRun(intent: Intent) {
        val run = synchronized(activeLock) { active }
        if (run == null) {
            logEvent(
                JSONObject()
                    .put("event", "observe_without_active_run")
                    .put("run_id", intent.getStringExtra("run_id")),
            )
            stopAfterTerminal()
            return
        }
        val snapshot = session.snapshot()
        appendEvent(
            run,
            JSONObject()
                .put("event", "activity_observed")
                .put("state", snapshot.state.name)
                .put("task_alive", snapshot.taskAlive)
                .put(
                    "buffered_payload_bytes",
                    snapshot.bufferedPayloadBytes.toLong(),
                )
                .put("requested_bytes", snapshot.requestedBytes.toLong())
                .put("received_bytes", snapshot.receivedBytes.toLong())
                .put("stored_bytes", snapshot.storedBytes.toLong()),
        )
    }

    private fun completeRun(
        run: ActiveRun,
        joined: JoinResult,
        completionSource: String,
    ) {
        if (!run.completed.compareAndSet(false, true)) {
            return
        }
        val snapshot = session.snapshot()
        appendEvent(
            run,
            JSONObject()
                .put("event", "engine_terminal")
                .put("source", completionSource)
                .put("joined", joined.joined)
                .put("state", snapshot.state.name)
                .put("task_alive", snapshot.taskAlive),
        )
        val platform =
            if (run.saf != null) {
                val terminal = joined.terminal
                if (
                    joined.joined &&
                    terminal?.outcome == TerminalOutcome.SUCCEEDED
                ) {
                    try {
                        val completed = SafDocuments.persistCompleted(this, run.saf)
                        SafDocuments.bindRunId(this, run.runId)
                        completed.put("status", "AWAITING_RESTART")
                    } catch (error: Throwable) {
                        SafDocuments.cleanup(this, run.saf)
                        JSONObject()
                            .put("status", "FAILED")
                            .put("failure_kind", "PLATFORM_STORAGE")
                            .put("failure_message", error.toString())
                    }
                } else {
                    SafDocuments.cleanup(this, run.saf)
                    JSONObject().put("status", "INCOMPLETE")
                }
            } else {
                JSONObject().put("status", "PATH_BACKED")
            }
        val result =
            JSONObject()
                .put("schema", 1)
                .put("run_id", run.runId)
                .put("scenario", run.scenario)
                .put("interface_version", interfaceVersion())
                .put(
                    "elapsed_millis",
                    SystemClock.elapsedRealtime() - run.startedElapsed,
                )
                .put("joined", joined.joined)
                .put("start", startJson(run.startResult))
                .put("snapshot", snapshotJson(snapshot))
                .put("terminal", terminalJson(joined.terminal))
                .put("platform", platform)
                .put("device", deviceJson())
                .put("memory", memoryJson())
                .put("fd_count_before", run.fdCountBefore)
                .put("fd_count_after", fdCount())
                .put("artifacts", artifactsJson(run.root))
        writeResult(run.resultPath, result)
        synchronized(activeLock) {
            if (active === run) {
                active = null
            }
        }
        stopAfterTerminal()
    }

    private fun verifyCompletedRun(intent: Intent) {
        val runId = BootstrapContract.requireRunId(intent.getStringExtra("run_id"))
        val resultsRoot = File(filesDir, "results")
        check(resultsRoot.mkdirs() || resultsRoot.isDirectory)
        val resultPath = File(resultsRoot, "$runId-restart.json")
        val result =
            try {
                SafDocuments
                    .verifyAndCleanup(this, runId)
                    .put("schema", 1)
                    .put("run_id", runId)
                    .put("interface_version", interfaceVersion())
                    .put("device", deviceJson())
            } catch (error: Throwable) {
                JSONObject()
                    .put("schema", 1)
                    .put("run_id", runId)
                    .put("status", "FAILED")
                    .put("failure_kind", "PLATFORM_STORAGE")
                    .put("failure_message", error.toString())
                    .put("interface_version", interfaceVersion())
                    .put("device", deviceJson())
            }
        writeResult(resultPath, result)
        stopAfterTerminal()
    }

    private fun parseIndexes(value: String?): List<UInt> {
        if (value.isNullOrBlank()) {
            return emptyList()
        }
        return value.split(",").map { field ->
            val parsed = field.toLong()
            require(parsed in 0..UInt.MAX_VALUE.toLong()) {
                "file index is out of range"
            }
            parsed.toUInt()
        }
    }

    private fun createCollision(
        collision: String,
        root: File,
        outputPath: File,
        resultPath: File,
    ) {
        val sentinel = "RSTORRENT_SENTINEL:$collision".toByteArray()
        when (collision) {
            "" -> Unit
            "output" -> {
                check(outputPath.createNewFile())
                outputPath.writeBytes(sentinel)
            }
            "staging" -> {
                val staging = File(root, ".downloaded.rstorrent-staging")
                check(staging.mkdir())
                File(staging, "sentinel").writeBytes(sentinel)
            }
            "part" -> {
                val part = File(root, ".downloaded.rstorrent-parts")
                check(part.createNewFile())
                part.writeBytes(sentinel)
            }
            "result" -> {
                check(resultPath.createNewFile())
                resultPath.writeBytes(sentinel)
            }
            else -> error("unknown collision profile $collision")
        }
    }

    private fun writeRejectedIfPossible(runId: String, reason: String) {
        val safeRunId =
            try {
                BootstrapContract.requireRunId(runId)
            } catch (error: IllegalArgumentException) {
                Log.e(TAG, "cannot write rejection for invalid run ID", error)
                return
            }
        val resultsRoot = File(filesDir, "results")
        if (!resultsRoot.exists() && !resultsRoot.mkdirs()) {
            Log.e(TAG, "cannot create results directory")
            return
        }
        val resultPath = File(resultsRoot, "$safeRunId.json")
        if (resultPath.exists()) {
            logEvent(
                JSONObject()
                    .put("event", "result_collision_preserved")
                    .put("run_id", safeRunId)
                    .put("reason", reason),
            )
            return
        }
        writeRejected(resultPath, safeRunId, "rejected", reason)
    }

    private fun writeRejected(
        path: File,
        runId: String,
        scenario: String,
        reason: String,
    ) {
        writeResult(
            path,
            JSONObject()
                .put("schema", 1)
                .put("run_id", runId)
                .put("scenario", scenario)
                .put("status", "REJECTED")
                .put("reason", reason)
                .put("interface_version", BootstrapContract.EXPECTED_INTERFACE)
                .put("device", deviceJson()),
        )
    }

    private fun writeResult(path: File, value: JSONObject) {
        try {
            check(!path.exists()) { "result path already exists" }
            val temporary = File(path.parentFile, ".${path.name}.tmp")
            check(temporary.createNewFile()) { "result temporary path exists" }
            FileOutputStream(temporary).use { output ->
                output.write(value.toString().toByteArray(Charsets.UTF_8))
                output.fd.sync()
            }
            check(temporary.renameTo(path)) { "result publication failed" }
            Log.i(TAG, "RSTORRENT_RESULT ${value}")
        } catch (error: Throwable) {
            Log.e(TAG, "result publication failed for ${path.name}", error)
        }
    }

    private fun appendEvent(run: ActiveRun, event: JSONObject) {
        event.put("elapsed_realtime", SystemClock.elapsedRealtime())
        synchronized(run) {
            run.eventsPath.appendText("${event}\n")
        }
        logEvent(event)
    }

    private fun logEvent(event: JSONObject) {
        Log.i(TAG, "RSTORRENT_EVENT $event")
    }

    private fun startJson(result: StartResult): JSONObject =
        JSONObject()
            .put("disposition", result.disposition.name)
            .put("generation", result.generation.toLong())
            .put("message", result.message)

    private fun snapshotJson(snapshot: SessionSnapshot): JSONObject =
        JSONObject()
            .put("state", snapshot.state.name)
            .put("generation", snapshot.generation.toLong())
            .put("task_alive", snapshot.taskAlive)
            .put(
                "cancellation_requested",
                snapshot.cancellationRequested,
            )
            .put(
                "buffered_payload_bytes",
                snapshot.bufferedPayloadBytes.toLong(),
            )
            .put(
                "payload_high_water",
                snapshot.payloadHighWater.toLong(),
            )
            .put(
                "outstanding_request_bytes",
                snapshot.outstandingRequestBytes.toLong(),
            )
            .put(
                "outstanding_request_high_water",
                snapshot.outstandingRequestHighWater.toLong(),
            )
            .put("requested_bytes", snapshot.requestedBytes.toLong())
            .put("received_bytes", snapshot.receivedBytes.toLong())
            .put("stored_bytes", snapshot.storedBytes.toLong())

    private fun terminalJson(terminal: TerminalResult?): Any {
        if (terminal == null) {
            return JSONObject.NULL
        }
        return JSONObject()
            .put("outcome", terminal.outcome.name)
            .put("failure_kind", terminal.failureKind?.name)
            .put("failure_message", terminal.failureMessage)
            .put("elapsed_millis", terminal.elapsedMillis.toLong())
            .put("report", reportJson(terminal.report))
    }

    private fun reportJson(report: EngineReport?): Any {
        if (report == null) {
            return JSONObject.NULL
        }
        return JSONObject()
            .put("info_hash", report.infoHashHex)
            .put("final_piece_hash", report.finalPieceHashHex)
            .put("bytes_written", report.bytesWritten.toLong())
            .put("block_count", report.blockCount.toLong())
            .put("payload_limit", report.payloadLimit.toLong())
            .put(
                "payload_high_water",
                report.payloadHighWater.toLong(),
            )
            .put(
                "outstanding_request_limit",
                report.outstandingRequestLimit.toLong(),
            )
            .put(
                "outstanding_request_high_water",
                report.outstandingRequestHighWater.toLong(),
            )
            .put("active_piece_limit", report.activePieceLimit.toLong())
            .put(
                "verification_buffer",
                report.verificationBuffer.toLong(),
            )
            .put("piece_count", report.pieceCount.toLong())
            .put(
                "verified_piece_count",
                report.verifiedPieceCount.toLong(),
            )
            .put(
                "skipped_piece_count",
                report.skippedPieceCount.toLong(),
            )
            .put(
                "selected_file_bytes",
                report.selectedFileBytes.toLong(),
            )
            .put(
                "skipped_file_bytes",
                report.skippedFileBytes.toLong(),
            )
            .put("padding_bytes", report.paddingBytes.toLong())
            .put(
                "selected_written_bytes",
                report.selectedWrittenBytes.toLong(),
            )
            .put(
                "part_written_bytes",
                report.partWrittenBytes.toLong(),
            )
            .put(
                "part_slots",
                report.partSlots.toLong(),
            )
            .put("part_reopened", report.partReopened)
            .put("part_path", report.partPath)
    }

    private fun deviceJson(): JSONObject =
        JSONObject()
            .put("model", Build.MODEL)
            .put("device", Build.DEVICE)
            .put("api", Build.VERSION.SDK_INT)
            .put("fingerprint", Build.FINGERPRINT)
            .put("abis", JSONArray(Build.SUPPORTED_ABIS.toList()))

    private fun memoryJson(): JSONObject {
        val memory = Debug.MemoryInfo()
        Debug.getMemoryInfo(memory)
        val runtime = Runtime.getRuntime()
        return JSONObject()
            .put(
                "java_heap_used",
                runtime.totalMemory() - runtime.freeMemory(),
            )
            .put(
                "native_heap_allocated",
                Debug.getNativeHeapAllocatedSize(),
            )
            .put("total_pss_kib", memory.totalPss)
            .put("dalvik_pss_kib", memory.dalvikPss)
            .put("native_pss_kib", memory.nativePss)
    }

    private fun artifactsJson(root: File): JSONArray {
        val artifacts = JSONArray()
        if (!root.exists()) {
            return artifacts
        }
        root.walkTopDown().take(128).forEach { path ->
            artifacts.put(path.relativeTo(root).path.ifEmpty { "." })
        }
        return artifacts
    }

    private fun fdCount(): Int = File("/proc/self/fd").list()?.size ?: -1

    private fun createNotificationChannel() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                getString(R.string.engine_bootstrap_name),
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    private fun notification(text: String): Notification =
        Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle(getString(R.string.engine_notification_title))
            .setContentText(text)
            .setOngoing(true)
            .build()

    private fun updateNotification(text: String) {
        getSystemService(NotificationManager::class.java)
            .notify(NOTIFICATION_ID, notification(text))
    }

    private fun stopAfterTerminal() {
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    companion object {
        private const val TAG = "RSTorrentBootstrap"
        private const val CHANNEL_ID = "rstorrent-bootstrap"
        private const val NOTIFICATION_ID = 104
    }
}
