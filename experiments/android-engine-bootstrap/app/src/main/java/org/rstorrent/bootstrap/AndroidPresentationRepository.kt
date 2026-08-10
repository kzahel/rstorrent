package org.rstorrent.bootstrap

import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.rstorrent.bootstrap.uniffi.AndroidApplicationClient
import org.rstorrent.bootstrap.uniffi.AndroidViewSubscription
import org.rstorrent.session.uniffi.CatalogPageRequest
import org.rstorrent.session.uniffi.DeliveryPolicy
import org.rstorrent.session.uniffi.DiagnosticCategory
import org.rstorrent.session.uniffi.DiagnosticFilter
import org.rstorrent.session.uniffi.DiagnosticProfile
import org.rstorrent.session.uniffi.DiagnosticSeverity
import org.rstorrent.session.uniffi.SpeedMetric
import org.rstorrent.session.uniffi.SpeedRange
import org.rstorrent.session.uniffi.SubscriptionSpec
import org.rstorrent.session.uniffi.ViewProjection
import org.rstorrent.session.uniffi.ViewSelector
import org.rstorrent.session.uniffi.ViewUpdate

/** Owns the bounded set of subscriptions that feed the Android product surface. */
internal class AndroidPresentationRepository(
    private val scope: CoroutineScope,
    private val state: MutableStateFlow<ProductState>,
    private val stopped: AtomicBoolean,
    private val onUpdate: (ViewUpdate, ProductState, Boolean) -> Unit,
    private val onError: (Throwable) -> Unit,
) {
    private val ownership = Mutex()
    private lateinit var client: AndroidApplicationClient
    private var list: OwnedSubscription? = null
    private val detail = mutableListOf<OwnedSubscription>()
    private var global: OwnedSubscription? = null
    private var diagnostics: OwnedSubscription? = null
    private var selectedTorrent: String? = null
    private var diagnosticProfile = DiagnosticProfile.NORMAL
    private var diagnosticSeverity = DiagnosticSeverity.INFO
    private var diagnosticCategories: List<DiagnosticCategory> = emptyList()
    private var diagnosticTorrentOnly = false

    suspend fun start(client: AndroidApplicationClient) {
        ownership.withLock {
            this.client = client
            list =
                subscribe(
                    SubscriptionSpec(
                        ViewSelector.TorrentList,
                        ViewProjection.SUMMARY,
                        DeliveryPolicy(250U, 256U * 1024U),
                        null,
                        null,
                    ),
                    driveSaf = true,
                )
            replaceDiagnostics()
        }
    }

    fun selectTorrent(torrentId: String) {
        scope.launch {
            ownership.withLock {
                if (selectedTorrent == torrentId) return@withLock
                selectedTorrent = torrentId
                state.update { it.copy(selectedTorrent = torrentId) }
                if (diagnosticTorrentOnly) replaceDiagnostics()
            }
        }
    }

    fun presentTorrent(
        torrentId: String,
        presentation: TorrentPresentation,
    ) {
        scope.launch {
            ownership.withLock {
                selectedTorrent = torrentId
                closeAll(detail)
                state.update {
                    it.copy(
                        selectedTorrent = torrentId,
                        files = it.files.filterKeys { id -> id == torrentId },
                        trackers = it.trackers.filterKeys { id -> id == torrentId },
                        peers = it.peers.filterKeys { id -> id == torrentId },
                        swarms = it.swarms.filterKeys { id -> id == torrentId },
                    )
                }
                detail += torrentSpecs(torrentId, presentation).map { subscribe(it, false) }
                if (diagnosticTorrentOnly) replaceDiagnostics()
            }
        }
    }

    fun clearTorrent(torrentId: String) {
        scope.launch {
            ownership.withLock {
                if (selectedTorrent != torrentId) return@withLock
                closeAll(detail)
            }
        }
    }

    fun presentCatalogPage(
        torrentId: String,
        presentation: TorrentPresentation,
        offset: UInt,
    ) {
        require(
            presentation == TorrentPresentation.FILES ||
                presentation == TorrentPresentation.TRACKERS,
        ) { "only paged catalog presentations accept an offset" }
        scope.launch {
            ownership.withLock {
                selectedTorrent = torrentId
                closeAll(detail)
                val projection =
                    if (presentation == TorrentPresentation.FILES) {
                        ViewProjection.FILES
                    } else {
                        ViewProjection.TRACKERS
                    }
                detail += subscribe(catalogSpec(torrentId, projection, offset), false)
            }
        }
    }

    fun presentGlobal(presentation: GlobalPresentation) {
        scope.launch {
            ownership.withLock {
                global?.close()
                global =
                    when (presentation) {
                        GlobalPresentation.NONE -> null
                        GlobalPresentation.SPEED -> subscribe(speedSpec(), false)
                        GlobalPresentation.DHT -> subscribe(dhtSpec(), false)
                    }
            }
        }
    }

    fun configureDiagnostics(
        profile: DiagnosticProfile,
        severity: DiagnosticSeverity,
        categories: List<DiagnosticCategory>,
        torrentOnly: Boolean,
    ) {
        scope.launch {
            ownership.withLock {
                diagnosticProfile = profile
                diagnosticSeverity = severity
                diagnosticCategories = categories
                diagnosticTorrentOnly = torrentOnly
                if (::client.isInitialized) replaceDiagnostics()
            }
        }
    }

    suspend fun close() {
        ownership.withLock {
            list?.close()
            list = null
            closeAll(detail)
            global?.close()
            global = null
            diagnostics?.close()
            diagnostics = null
        }
    }

    private suspend fun replaceDiagnostics() {
        diagnostics?.close()
        val selector =
            if (diagnosticTorrentOnly && selectedTorrent != null) {
                ViewSelector.Torrent(requireNotNull(selectedTorrent))
            } else {
                ViewSelector.TorrentList
            }
        diagnostics =
            subscribe(
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
                false,
            )
    }

    private suspend fun subscribe(
        spec: SubscriptionSpec,
        driveSaf: Boolean,
    ): OwnedSubscription {
        val subscription = client.subscribe(spec)
        val job =
            scope.launch {
                try {
                    while (true) {
                        val update = subscription.nextUpdate() ?: break
                        try {
                            var reduced: ProductState? = null
                            state.update { current ->
                                ProductStateReducer.reduce(current, update).also { reduced = it }
                            }
                            onUpdate(update, requireNotNull(reduced), driveSaf)
                        } catch (_: ViewResetRequiredException) {
                            state.update { it.copy(diagnosticResets = it.diagnosticResets + 1UL) }
                            subscription.resync()
                        }
                    }
                } catch (error: Throwable) {
                    if (!stopped.get() && error !is CancellationException) onError(error)
                } finally {
                    subscription.close()
                }
            }
        return OwnedSubscription(subscription, job)
    }

    private fun closeAll(subscriptions: MutableList<OwnedSubscription>) {
        subscriptions.forEach(OwnedSubscription::close)
        subscriptions.clear()
    }

    private fun torrentSpecs(
        torrentId: String,
        presentation: TorrentPresentation,
    ): List<SubscriptionSpec> =
        when (presentation) {
            TorrentPresentation.SUMMARY -> emptyList()
            TorrentPresentation.FILES ->
                listOf(catalogSpec(torrentId, ViewProjection.FILES, 0U))
            TorrentPresentation.TRACKERS ->
                listOf(catalogSpec(torrentId, ViewProjection.TRACKERS, 0U))
            TorrentPresentation.PEERS ->
                listOf(
                    torrentSpec(torrentId, ViewProjection.PEERS, 250U),
                    torrentSpec(torrentId, ViewProjection.SWARM, 500U),
                )
            TorrentPresentation.PIECES ->
                listOf(
                    torrentSpec(torrentId, ViewProjection.PIECE_ACTIVITY, 100U),
                    SubscriptionSpec(
                        ViewSelector.TorrentList,
                        ViewProjection.DISK,
                        DeliveryPolicy(250U, 512U * 1024U),
                        null,
                        null,
                    ),
                )
        }

    private fun torrentSpec(
        torrentId: String,
        projection: ViewProjection,
        interval: UInt,
    ) = SubscriptionSpec(
        ViewSelector.Torrent(torrentId),
        projection,
        DeliveryPolicy(interval, 512U * 1024U),
        null,
        null,
    )

    private fun catalogSpec(
        torrentId: String,
        projection: ViewProjection,
        offset: UInt,
    ) = SubscriptionSpec(
        ViewSelector.Torrent(torrentId),
        projection,
        DeliveryPolicy(250U, 512U * 1024U),
        null,
        CatalogPageRequest(offset, 1_024U),
    )

    private fun speedSpec() =
        SubscriptionSpec(
            ViewSelector.SessionSpeed(
                SpeedRange.MINUTES2,
                listOf(
                    SpeedMetric.PAYLOAD_RECEIVED,
                    SpeedMetric.PAYLOAD_UPLOADED,
                    SpeedMetric.STAGED_WRITE,
                    SpeedMetric.PAYLOAD_VERIFIED,
                    SpeedMetric.PEER_WIRE_RECEIVED,
                    SpeedMetric.PEER_WIRE_SENT,
                ),
            ),
            ViewProjection.SPEED,
            DeliveryPolicy(500U, 512U * 1024U),
            null,
            null,
        )

    private fun dhtSpec() =
        SubscriptionSpec(
            ViewSelector.SessionDht,
            ViewProjection.DHT,
            DeliveryPolicy(500U, 512U * 1024U),
            null,
            null,
        )

    private data class OwnedSubscription(
        val subscription: AndroidViewSubscription,
        val job: Job,
    ) {
        fun close() {
            job.cancel()
            subscription.close()
        }
    }
}
