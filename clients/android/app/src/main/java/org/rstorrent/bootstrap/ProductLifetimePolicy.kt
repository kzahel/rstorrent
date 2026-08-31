package org.rstorrent.bootstrap

import org.rstorrent.session.uniffi.ProgressReason
import org.rstorrent.session.uniffi.TorrentOperationalState
import org.rstorrent.session.uniffi.TorrentView

internal const val PRODUCT_LIFETIME_SETTLE_MILLIS = 2_000L
internal const val PRODUCT_LIFETIME_RESYNC_MILLIS = 5_000L
internal const val PRODUCT_LIFETIME_STARTUP_MILLIS = 30_000L
internal const val PRODUCT_COMPANION_RECONNECT_MILLIS = 60_000L

internal enum class ProductBackgroundCompletionPolicy(val persistedValue: String) {
    STOP_WHEN_DOWNLOADS_COMPLETE("stop_when_downloads_complete"),
    KEEP_SEEDING("keep_seeding"),
    ;

    companion object {
        fun decode(value: String?): ProductBackgroundCompletionPolicy =
            entries.firstOrNull { it.persistedValue == value } ?: STOP_WHEN_DOWNLOADS_COMPLETE
    }
}

internal data class ProductLifecyclePreferences(
    val backgroundDownloadsEnabled: Boolean = false,
    val completionPolicy: ProductBackgroundCompletionPolicy =
        ProductBackgroundCompletionPolicy.STOP_WHEN_DOWNLOADS_COMPLETE,
) {
    val keepSeedingEnabled: Boolean
        get() =
            backgroundDownloadsEnabled &&
                completionPolicy == ProductBackgroundCompletionPolicy.KEEP_SEEDING
}

internal sealed interface ProductLifecyclePreferenceResult {
    data class Applied(val preferences: ProductLifecyclePreferences) :
        ProductLifecyclePreferenceResult

    data object Failed : ProductLifecyclePreferenceResult
}

internal fun persistProductLifecyclePreferences(
    current: ProductLifecyclePreferences,
    requested: ProductLifecyclePreferences,
    persist: (ProductLifecyclePreferences) -> Boolean,
): ProductLifecyclePreferenceResult {
    if (requested == current) return ProductLifecyclePreferenceResult.Applied(current)
    if (!persist(requested)) return ProductLifecyclePreferenceResult.Failed
    return ProductLifecyclePreferenceResult.Applied(requested)
}

internal data class ProductLifetimeWork(
    val starting: Int = 0,
    val downloading: Int = 0,
    val checking: Int = 0,
    val waitingForUnmeteredNetwork: Int = 0,
    val seeding: Int = 0,
) {
    init {
        require(starting >= 0)
        require(downloading >= 0)
        require(checking >= 0)
        require(waitingForUnmeteredNetwork >= 0)
        require(seeding >= 0)
    }

    val hasDownloadWork: Boolean
        get() = starting > 0 || downloading > 0 || checking > 0

    val isEmpty: Boolean
        get() = !hasDownloadWork && waitingForUnmeteredNetwork == 0 && seeding == 0
}

internal data class ProductLifetimeTorrentFacts(
    val operationalState: TorrentOperationalState,
    val progressReason: ProgressReason,
    val archived: Boolean = false,
    val removalPending: Boolean = false,
    val awaitingFileSelection: Boolean = false,
    val metadataAvailable: Boolean = true,
)

internal fun classifyProductLifetimeTorrentViews(
    torrents: Collection<TorrentView>,
): ProductLifetimeWork =
    classifyProductLifetimeWork(
        torrents.map {
            ProductLifetimeTorrentFacts(
                operationalState = it.operationalState,
                progressReason = it.progress.reason,
                archived = it.archived,
                removalPending = it.removalState != null,
                awaitingFileSelection = it.awaitingFileSelection,
                metadataAvailable = it.metadataAvailable,
            )
        },
    )

internal fun classifyProductLifetimeWork(
    torrents: Collection<ProductLifetimeTorrentFacts>,
): ProductLifetimeWork {
    var starting = 0
    var downloading = 0
    var checking = 0
    var waitingForUnmeteredNetwork = 0
    var seeding = 0
    torrents.forEach { torrent ->
        if (
            torrent.archived ||
            torrent.removalPending ||
            (torrent.awaitingFileSelection && torrent.metadataAvailable)
        ) {
            return@forEach
        }
        when (torrent.operationalState) {
            TorrentOperationalState.STARTING -> starting += 1
            TorrentOperationalState.DOWNLOADING -> downloading += 1
            TorrentOperationalState.CHECKING -> checking += 1
            TorrentOperationalState.SEEDING -> seeding += 1
            TorrentOperationalState.QUEUED,
            TorrentOperationalState.STOPPING,
            TorrentOperationalState.PAUSED,
            TorrentOperationalState.ERROR,
            -> Unit
        }
        if (torrent.progressReason == ProgressReason.WAITING_FOR_UNMETERED_NETWORK) {
            waitingForUnmeteredNetwork += 1
        }
    }
    return ProductLifetimeWork(
        starting = starting,
        downloading = downloading,
        checking = checking,
        waitingForUnmeteredNetwork = waitingForUnmeteredNetwork,
        seeding = seeding,
    )
}

internal enum class ProductLifetimeRetentionReason {
    INTERACTIVE,
    PLATFORM_WORKFLOW,
    ACTIVE_DOWNLOAD,
    WAITING_FOR_UNMETERED_NETWORK,
    BACKGROUND_SEEDING,
    CHROMEOS_COMPANION,
    CHROMEOS_RECONNECT_GRACE,
}

internal enum class ProductLifetimeWaitReason {
    STARTUP,
    VIEW_RESYNC,
    VISIBILITY_SETTLE,
    WORK_SETTLE,
}

internal enum class ProductLifetimeStopReason {
    IDLE,
    NOTIFICATION_INELIGIBLE,
    AUTHORITATIVE_STATE_UNAVAILABLE,
    EXPLICIT_STOP,
    DATA_SYNC_TIMEOUT,
    INITIALIZATION_FAILED,
    FOREGROUND_PROMOTION_FAILED,
    REVISION_EXHAUSTED,
}

internal sealed interface ProductLifetimeDecision {
    data class Retain(
        val reason: ProductLifetimeRetentionReason,
        val foregroundRequired: Boolean,
        val stickyAllowed: Boolean,
    ) : ProductLifetimeDecision

    data class Wait(
        val reason: ProductLifetimeWaitReason,
        val deadlineMillis: Long,
        val foregroundRequired: Boolean,
    ) : ProductLifetimeDecision

    data class Stop(val reason: ProductLifetimeStopReason) : ProductLifetimeDecision
}

internal data class ProductLifetimeFacts(
    val revision: Long,
    val nowMillis: Long,
    val activityVisible: Boolean,
    val interactionLeaseCount: Int,
    val notificationEligible: Boolean,
    val preferences: ProductLifecyclePreferences,
    val work: ProductLifetimeWork?,
    val companionConnections: Int,
    val startupDeadlineMillis: Long? = null,
    val resyncDeadlineMillis: Long? = null,
    val settleDeadlineMillis: Long? = null,
    val settleReason: ProductLifetimeWaitReason = ProductLifetimeWaitReason.WORK_SETTLE,
    val companionGraceDeadlineMillis: Long? = null,
    val terminalReason: ProductLifetimeStopReason? = null,
) {
    init {
        require(revision > 0) { "lifetime revision must be nonzero" }
        require(nowMillis >= 0) { "monotonic time cannot be negative" }
        require(interactionLeaseCount >= 0) { "interaction lease count cannot be negative" }
        require(companionConnections >= 0) { "companion connection count cannot be negative" }
        require(
            settleReason == ProductLifetimeWaitReason.VISIBILITY_SETTLE ||
                settleReason == ProductLifetimeWaitReason.WORK_SETTLE,
        ) { "settle deadline requires a settle reason" }
    }
}

internal object ProductLifetimePolicy {
    fun reduce(facts: ProductLifetimeFacts): ProductLifetimeDecision {
        facts.terminalReason?.let { return ProductLifetimeDecision.Stop(it) }

        if (facts.activityVisible) {
            return ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.INTERACTIVE,
                foregroundRequired = false,
                stickyAllowed = false,
            )
        }
        if (facts.interactionLeaseCount > 0) {
            return ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.PLATFORM_WORKFLOW,
                foregroundRequired = true,
                stickyAllowed = false,
            )
        }
        if (facts.work == null) {
            activeDeadline(facts.nowMillis, facts.startupDeadlineMillis)?.let {
                return ProductLifetimeDecision.Wait(
                    ProductLifetimeWaitReason.STARTUP,
                    it,
                    foregroundRequired = true,
                )
            }
        }
        if (!facts.notificationEligible) {
            return ProductLifetimeDecision.Stop(
                ProductLifetimeStopReason.NOTIFICATION_INELIGIBLE,
            )
        }

        val work = facts.work
        if (work != null && facts.preferences.backgroundDownloadsEnabled) {
            if (work.hasDownloadWork) {
                return ProductLifetimeDecision.Retain(
                    ProductLifetimeRetentionReason.ACTIVE_DOWNLOAD,
                    foregroundRequired = true,
                    stickyAllowed = true,
                )
            }
            if (work.waitingForUnmeteredNetwork > 0) {
                return ProductLifetimeDecision.Retain(
                    ProductLifetimeRetentionReason.WAITING_FOR_UNMETERED_NETWORK,
                    foregroundRequired = true,
                    stickyAllowed = true,
                )
            }
            if (facts.preferences.keepSeedingEnabled && work.seeding > 0) {
                return ProductLifetimeDecision.Retain(
                    ProductLifetimeRetentionReason.BACKGROUND_SEEDING,
                    foregroundRequired = true,
                    stickyAllowed = true,
                )
            }
        }

        if (facts.companionConnections > 0) {
            return ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.CHROMEOS_COMPANION,
                foregroundRequired = true,
                stickyAllowed = true,
            )
        }
        activeDeadline(facts.nowMillis, facts.companionGraceDeadlineMillis)?.let {
            return ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.CHROMEOS_RECONNECT_GRACE,
                foregroundRequired = true,
                stickyAllowed = true,
            )
        }
        if (work == null) {
            activeDeadline(facts.nowMillis, facts.resyncDeadlineMillis)?.let {
                return ProductLifetimeDecision.Wait(
                    ProductLifetimeWaitReason.VIEW_RESYNC,
                    it,
                    foregroundRequired = true,
                )
            }
            return ProductLifetimeDecision.Stop(
                ProductLifetimeStopReason.AUTHORITATIVE_STATE_UNAVAILABLE,
            )
        }
        activeDeadline(facts.nowMillis, facts.settleDeadlineMillis)?.let {
            return ProductLifetimeDecision.Wait(
                facts.settleReason,
                it,
                foregroundRequired = true,
            )
        }
        return ProductLifetimeDecision.Stop(ProductLifetimeStopReason.IDLE)
    }

    private fun activeDeadline(
        nowMillis: Long,
        deadlineMillis: Long?,
    ): Long? = deadlineMillis?.takeIf { nowMillis < it }
}
