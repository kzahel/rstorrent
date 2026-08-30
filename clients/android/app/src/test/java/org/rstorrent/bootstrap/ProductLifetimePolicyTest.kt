package org.rstorrent.bootstrap

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.rstorrent.session.uniffi.ProgressReason
import org.rstorrent.session.uniffi.TorrentOperationalState

class ProductLifetimePolicyTest {
    @Test
    fun preferencesDefaultOffAndStopOnCompletion() {
        val preferences = ProductLifecyclePreferences()

        assertFalse(preferences.backgroundDownloadsEnabled)
        assertEquals(
            ProductBackgroundCompletionPolicy.STOP_WHEN_DOWNLOADS_COMPLETE,
            preferences.completionPolicy,
        )
        assertFalse(preferences.keepSeedingEnabled)
        assertEquals(
            ProductBackgroundCompletionPolicy.STOP_WHEN_DOWNLOADS_COMPLETE,
            ProductBackgroundCompletionPolicy.decode("unknown"),
        )
    }

    @Test
    fun disablingBackgroundRetainsTheSeedingChoice() {
        val enabled =
            ProductLifecyclePreferences(
                backgroundDownloadsEnabled = true,
                completionPolicy = ProductBackgroundCompletionPolicy.KEEP_SEEDING,
            )
        val disabled = enabled.copy(backgroundDownloadsEnabled = false)

        assertFalse(disabled.keepSeedingEnabled)
        assertEquals(ProductBackgroundCompletionPolicy.KEEP_SEEDING, disabled.completionPolicy)
    }

    @Test
    fun persistenceMustSucceedBeforeThePolicyChanges() {
        val current = ProductLifecyclePreferences()
        val requested = current.copy(backgroundDownloadsEnabled = true)
        var writes = 0

        assertSame(
            ProductLifecyclePreferenceResult.Failed,
            persistProductLifecyclePreferences(current, requested) {
                writes += 1
                false
            },
        )
        assertEquals(1, writes)
        assertEquals(
            ProductLifecyclePreferenceResult.Applied(current),
            persistProductLifecyclePreferences(current, current) {
                writes += 1
                true
            },
        )
        assertEquals(1, writes)
    }

    @Test
    fun visibleActivityAlwaysRetainsInteractiveWithoutStickyAdmission() {
        val decision = reduce(activityVisible = true, notificationEligible = false)

        assertEquals(
            ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.INTERACTIVE,
                foregroundRequired = false,
                stickyAllowed = false,
            ),
            decision,
        )
    }

    @Test
    fun boundedWorkflowRetainsVisibleOnlyOperation() {
        val decision =
            reduce(
                interactionLeaseCount = 1,
                notificationEligible = false,
            )

        assertEquals(
            ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.PLATFORM_WORKFLOW,
                foregroundRequired = true,
                stickyAllowed = false,
            ),
            decision,
        )
    }

    @Test
    fun notificationIneligibilityStopsEveryUnattendedReason() {
        val active = ProductLifetimeWork(downloading = 1, seeding = 1)

        assertEquals(
            ProductLifetimeDecision.Stop(
                ProductLifetimeStopReason.NOTIFICATION_INELIGIBLE,
            ),
            reduce(
                notificationEligible = false,
                preferences = enabledPreferences(keepSeeding = true),
                work = active,
                companionConnections = 2,
            ),
        )
    }

    @Test
    fun defaultOffActiveWorkSettlesThenStops() {
        assertEquals(
            ProductLifetimeDecision.Wait(
                ProductLifetimeWaitReason.WORK_SETTLE,
                3_000L,
                foregroundRequired = true,
            ),
            reduce(
                nowMillis = 1_000L,
                work = ProductLifetimeWork(downloading = 1),
                settleDeadlineMillis = 3_000L,
            ),
        )
        assertEquals(
            ProductLifetimeDecision.Stop(ProductLifetimeStopReason.IDLE),
            reduce(
                nowMillis = 3_000L,
                work = ProductLifetimeWork(downloading = 1),
                settleDeadlineMillis = 3_000L,
            ),
        )
    }

    @Test
    fun enabledDownloadCheckingAndStartingAreStickyBackgroundReasons() {
        listOf(
            ProductLifetimeWork(starting = 1),
            ProductLifetimeWork(downloading = 1),
            ProductLifetimeWork(checking = 1),
        ).forEach { work ->
            assertEquals(
                ProductLifetimeDecision.Retain(
                    ProductLifetimeRetentionReason.ACTIVE_DOWNLOAD,
                    foregroundRequired = true,
                    stickyAllowed = true,
                ),
                reduce(preferences = enabledPreferences(), work = work),
            )
        }
    }

    @Test
    fun unmeteredWaitingIsASeparateStickyReason() {
        assertEquals(
            ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.WAITING_FOR_UNMETERED_NETWORK,
                foregroundRequired = true,
                stickyAllowed = true,
            ),
            reduce(
                preferences = enabledPreferences(),
                work = ProductLifetimeWork(waitingForUnmeteredNetwork = 1),
            ),
        )
    }

    @Test
    fun seedingRequiresBothBackgroundAndKeepSeeding() {
        val work = ProductLifetimeWork(seeding = 1)
        assertEquals(
            ProductLifetimeDecision.Stop(ProductLifetimeStopReason.IDLE),
            reduce(preferences = enabledPreferences(), work = work),
        )
        assertEquals(
            ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.BACKGROUND_SEEDING,
                foregroundRequired = true,
                stickyAllowed = true,
            ),
            reduce(preferences = enabledPreferences(keepSeeding = true), work = work),
        )
    }

    @Test
    fun companionIsIndependentFromBackgroundPreference() {
        assertEquals(
            ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.CHROMEOS_COMPANION,
                foregroundRequired = true,
                stickyAllowed = true,
            ),
            reduce(companionConnections = 1),
        )
        assertEquals(
            ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.CHROMEOS_RECONNECT_GRACE,
                foregroundRequired = true,
                stickyAllowed = true,
            ),
            reduce(nowMillis = 1_000L, companionGraceDeadlineMillis = 61_000L),
        )
        assertEquals(
            ProductLifetimeDecision.Stop(ProductLifetimeStopReason.IDLE),
            reduce(nowMillis = 61_000L, companionGraceDeadlineMillis = 61_000L),
        )
        assertEquals(
            ProductLifetimeDecision.Retain(
                ProductLifetimeRetentionReason.CHROMEOS_COMPANION,
                foregroundRequired = true,
                stickyAllowed = true,
            ),
            reduce(work = null, companionConnections = 1),
        )
    }

    @Test
    fun startupAndResyncAreBoundedAndFailClosed() {
        assertEquals(
            ProductLifetimeDecision.Wait(
                ProductLifetimeWaitReason.STARTUP,
                30_000L,
                foregroundRequired = true,
            ),
            reduce(work = null, startupDeadlineMillis = 30_000L),
        )
        assertEquals(
            ProductLifetimeDecision.Wait(
                ProductLifetimeWaitReason.VIEW_RESYNC,
                5_000L,
                foregroundRequired = true,
            ),
            reduce(work = null, resyncDeadlineMillis = 5_000L),
        )
        assertEquals(
            ProductLifetimeDecision.Stop(
                ProductLifetimeStopReason.AUTHORITATIVE_STATE_UNAVAILABLE,
            ),
            reduce(work = null),
        )
        assertEquals(
            ProductLifetimeDecision.Wait(
                ProductLifetimeWaitReason.STARTUP,
                30_000L,
                foregroundRequired = true,
            ),
            reduce(
                notificationEligible = false,
                work = null,
                startupDeadlineMillis = 30_000L,
            ),
        )
        assertEquals(
            ProductLifetimeDecision.Stop(
                ProductLifetimeStopReason.NOTIFICATION_INELIGIBLE,
            ),
            reduce(
                notificationEligible = false,
                work = null,
                resyncDeadlineMillis = 5_000L,
            ),
        )
    }

    @Test
    fun terminalFactsPrecedeVisibilityAndAllOtherReasons() {
        ProductLifetimeStopReason.entries.forEach { reason ->
            assertEquals(
                ProductLifetimeDecision.Stop(reason),
                reduce(
                    activityVisible = true,
                    preferences = enabledPreferences(keepSeeding = true),
                    work = ProductLifetimeWork(downloading = 1, seeding = 1),
                    companionConnections = 1,
                    terminalReason = reason,
                ),
            )
        }
    }

    @Test(expected = IllegalArgumentException::class)
    fun zeroRevisionIsRejected() {
        facts().copy(revision = 0)
    }

    @Test
    fun workCountsAreBoundedFactsNotRates() {
        val work = ProductLifetimeWork(starting = 1, downloading = 2, checking = 3)
        assertTrue(work.hasDownloadWork)
        assertFalse(work.isEmpty)
        assertTrue(ProductLifetimeWork().isEmpty)
    }

    @Test
    fun workClassifierUsesOnlyClosedAuthoritativeStates() {
        val rows =
            listOf(
                torrent(TorrentOperationalState.STARTING),
                torrent(TorrentOperationalState.DOWNLOADING),
                torrent(TorrentOperationalState.CHECKING),
                torrent(TorrentOperationalState.SEEDING),
                torrent(TorrentOperationalState.QUEUED),
                torrent(TorrentOperationalState.PAUSED),
                torrent(TorrentOperationalState.ERROR),
                torrent(
                    TorrentOperationalState.QUEUED,
                    ProgressReason.WAITING_FOR_UNMETERED_NETWORK,
                ),
                torrent(TorrentOperationalState.DOWNLOADING, archived = true),
                torrent(TorrentOperationalState.SEEDING, removalPending = true),
            )

        assertEquals(
            ProductLifetimeWork(
                starting = 1,
                downloading = 1,
                checking = 1,
                waitingForUnmeteredNetwork = 1,
                seeding = 1,
            ),
            classifyProductLifetimeWork(rows),
        )
    }

    private fun enabledPreferences(keepSeeding: Boolean = false) =
        ProductLifecyclePreferences(
            backgroundDownloadsEnabled = true,
            completionPolicy =
                if (keepSeeding) {
                    ProductBackgroundCompletionPolicy.KEEP_SEEDING
                } else {
                    ProductBackgroundCompletionPolicy.STOP_WHEN_DOWNLOADS_COMPLETE
                },
        )

    private fun torrent(
        state: TorrentOperationalState,
        reason: ProgressReason = ProgressReason.DISCOVERING_PEERS,
        archived: Boolean = false,
        removalPending: Boolean = false,
    ) = ProductLifetimeTorrentFacts(state, reason, archived, removalPending)

    private fun reduce(
        revision: Long = 1,
        nowMillis: Long = 0,
        activityVisible: Boolean = false,
        interactionLeaseCount: Int = 0,
        notificationEligible: Boolean = true,
        preferences: ProductLifecyclePreferences = ProductLifecyclePreferences(),
        work: ProductLifetimeWork? = ProductLifetimeWork(),
        companionConnections: Int = 0,
        startupDeadlineMillis: Long? = null,
        resyncDeadlineMillis: Long? = null,
        settleDeadlineMillis: Long? = null,
        companionGraceDeadlineMillis: Long? = null,
        terminalReason: ProductLifetimeStopReason? = null,
    ): ProductLifetimeDecision =
        ProductLifetimePolicy.reduce(
            facts(
                revision,
                nowMillis,
                activityVisible,
                interactionLeaseCount,
                notificationEligible,
                preferences,
                work,
                companionConnections,
                startupDeadlineMillis,
                resyncDeadlineMillis,
                settleDeadlineMillis,
                companionGraceDeadlineMillis,
                terminalReason,
            ),
        )

    @Suppress("LongParameterList")
    private fun facts(
        revision: Long = 1,
        nowMillis: Long = 0,
        activityVisible: Boolean = false,
        interactionLeaseCount: Int = 0,
        notificationEligible: Boolean = true,
        preferences: ProductLifecyclePreferences = ProductLifecyclePreferences(),
        work: ProductLifetimeWork? = ProductLifetimeWork(),
        companionConnections: Int = 0,
        startupDeadlineMillis: Long? = null,
        resyncDeadlineMillis: Long? = null,
        settleDeadlineMillis: Long? = null,
        companionGraceDeadlineMillis: Long? = null,
        terminalReason: ProductLifetimeStopReason? = null,
    ) =
        ProductLifetimeFacts(
            revision = revision,
            nowMillis = nowMillis,
            activityVisible = activityVisible,
            interactionLeaseCount = interactionLeaseCount,
            notificationEligible = notificationEligible,
            preferences = preferences,
            work = work,
            companionConnections = companionConnections,
            startupDeadlineMillis = startupDeadlineMillis,
            resyncDeadlineMillis = resyncDeadlineMillis,
            settleDeadlineMillis = settleDeadlineMillis,
            companionGraceDeadlineMillis = companionGraceDeadlineMillis,
            terminalReason = terminalReason,
        )
}
