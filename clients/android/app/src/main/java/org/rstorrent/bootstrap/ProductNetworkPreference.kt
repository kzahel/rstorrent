package org.rstorrent.bootstrap

import android.content.Context

internal object ProductNetworkPreference {
    private const val PREFERENCES = "product_network"
    private const val UNMETERED_NETWORKS_ONLY = "unmetered_networks_only"

    fun read(context: Context): Boolean =
        context
            .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getBoolean(UNMETERED_NETWORKS_ONLY, false)

    fun persist(
        context: Context,
        enabled: Boolean,
    ): Boolean =
        context
            .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putBoolean(UNMETERED_NETWORKS_ONLY, enabled)
            .commit()
}

data class ProductNetworkState(
    val unmeteredNetworksOnly: Boolean = false,
    val eligibility: AndroidNetworkEligibility = AndroidNetworkEligibility.UNRESTRICTED,
    val observationRevision: Long = 0,
    val callbackRegistered: Boolean = false,
    val preferenceError: ProductError? = null,
    val effectiveNetworkAllowed: Boolean? = null,
    val effectiveGeneration: String? = null,
    val runtimeError: ProductError? = null,
) {
    val currentTruth: ProductNetworkTruth
        get() =
            when {
                !unmeteredNetworksOnly -> ProductNetworkTruth.UNRESTRICTED
                eligibility == AndroidNetworkEligibility.UNRESTRICTED -> ProductNetworkTruth.UNMETERED
                eligibility == AndroidNetworkEligibility.WAITING_FOR_UNMETERED_NETWORK ->
                    ProductNetworkTruth.METERED
                eligibility == AndroidNetworkEligibility.WAITING_FOR_VALIDATED_INTERNET ->
                    ProductNetworkTruth.NO_VALIDATED_INTERNET
                eligibility == AndroidNetworkEligibility.WAITING_FOR_USABLE_NETWORK ->
                    ProductNetworkTruth.TEMPORARILY_UNAVAILABLE
                eligibility == AndroidNetworkEligibility.WAITING_FOR_DEFAULT_NETWORK ->
                    ProductNetworkTruth.TEMPORARILY_UNAVAILABLE
                else -> ProductNetworkTruth.CHECKING
            }
}

enum class ProductNetworkTruth {
    UNRESTRICTED,
    UNMETERED,
    METERED,
    NO_VALIDATED_INTERNET,
    TEMPORARILY_UNAVAILABLE,
    CHECKING,
}
