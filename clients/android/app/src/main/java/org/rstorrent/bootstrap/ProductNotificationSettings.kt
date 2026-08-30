package org.rstorrent.bootstrap

internal enum class ProductNotificationPreference {
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
