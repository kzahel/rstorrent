package org.rstorrent.bootstrap

import java.util.concurrent.atomic.AtomicBoolean

enum class ProductNotificationPreference {
    DOWNLOAD_COMPLETE,
    NEEDS_ATTENTION,
}

data class ProductNotificationPreferences(
    val downloadComplete: Boolean = true,
    val needsAttention: Boolean = true,
) {
    internal fun enabled(category: ProductNotificationCategory): Boolean =
        when (category) {
            ProductNotificationCategory.DOWNLOAD_COMPLETE -> downloadComplete
            ProductNotificationCategory.NEEDS_ATTENTION -> needsAttention
        }

    internal fun with(
        preference: ProductNotificationPreference,
        enabled: Boolean,
    ): ProductNotificationPreferences =
        when (preference) {
            ProductNotificationPreference.DOWNLOAD_COMPLETE -> copy(downloadComplete = enabled)
            ProductNotificationPreference.NEEDS_ATTENTION -> copy(needsAttention = enabled)
        }
}

internal sealed interface ProductNotificationPreferenceResult {
    data class Applied(val preferences: ProductNotificationPreferences) :
        ProductNotificationPreferenceResult

    data object Failed : ProductNotificationPreferenceResult
}

internal fun persistNotificationPreference(
    current: ProductNotificationPreferences,
    preference: ProductNotificationPreference,
    enabled: Boolean,
    persist: (ProductNotificationPreference, Boolean) -> Boolean,
): ProductNotificationPreferenceResult {
    if (current.with(preference, enabled) == current) {
        return ProductNotificationPreferenceResult.Applied(current)
    }
    if (!persist(preference, enabled)) return ProductNotificationPreferenceResult.Failed
    return ProductNotificationPreferenceResult.Applied(current.with(preference, enabled))
}

internal data class NotificationEligibility(
    val permissionGranted: Boolean,
    val appNotificationsEnabled: Boolean,
    val backgroundChannelEnabled: Boolean,
    val interactionLeaseCount: Int,
) {
    init {
        require(interactionLeaseCount >= 0) { "interaction lease count cannot be negative" }
    }

    val backgroundNotificationVisible: Boolean
        get() = permissionGranted && appNotificationsEnabled && backgroundChannelEnabled

    val visibleOnly: Boolean
        get() = !backgroundNotificationVisible

    val shouldStopOwner: Boolean
        get() = visibleOnly && interactionLeaseCount == 0
}

/** Process-local activity visibility; process death deliberately resets it to absent. */
internal object ProductActivityVisibility {
    private val visible = AtomicBoolean(false)
    private val ownership = Any()
    private var listener: ((Boolean) -> Unit)? = null

    fun setVisible(value: Boolean) {
        visible.set(value)
        synchronized(ownership) { listener }?.invoke(value)
    }

    fun attach(listener: (Boolean) -> Unit) {
        synchronized(ownership) {
            check(this.listener == null) { "activity visibility already has a service owner" }
            this.listener = listener
        }
        listener(visible.get())
    }

    fun detach() {
        synchronized(ownership) { listener = null }
    }

    internal fun resetForTest() {
        synchronized(ownership) { listener = null }
        visible.set(false)
    }
}

data class ProductNotificationState(
    val preferences: ProductNotificationPreferences = ProductNotificationPreferences(),
    val permissionGranted: Boolean = false,
    val appNotificationsEnabled: Boolean = true,
    val backgroundChannelEnabled: Boolean = true,
    val completionChannelEnabled: Boolean = true,
    val attentionChannelEnabled: Boolean = true,
    val interactionLeaseCount: Int = 0,
    val preferenceError: String? = null,
) {
    internal val eligibility: NotificationEligibility
        get() =
            NotificationEligibility(
                permissionGranted,
                appNotificationsEnabled,
                backgroundChannelEnabled,
                interactionLeaseCount,
            )
}

sealed interface ProductNotificationNavigation {
    val sequence: Long

    data class Torrent(
        override val sequence: Long,
        val torrentId: String,
    ) : ProductNotificationNavigation

    data class StorageRepair(
        override val sequence: Long,
        val rootId: String?,
    ) : ProductNotificationNavigation
}
