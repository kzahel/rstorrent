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
    val preferenceError: String? = null,
    val effectiveNetworkAllowed: Boolean? = null,
    val effectiveGeneration: String? = null,
    val runtimeError: String? = null,
) {
    val currentTruth: String
        get() =
            when {
                !unmeteredNetworksOnly -> "Unrestricted"
                eligibility == AndroidNetworkEligibility.UNRESTRICTED -> "Unmetered network"
                eligibility == AndroidNetworkEligibility.WAITING_FOR_UNMETERED_NETWORK ->
                    "Metered network"
                eligibility == AndroidNetworkEligibility.WAITING_FOR_VALIDATED_INTERNET ->
                    "No validated internet"
                eligibility == AndroidNetworkEligibility.WAITING_FOR_USABLE_NETWORK ->
                    "Network temporarily unavailable"
                eligibility == AndroidNetworkEligibility.WAITING_FOR_DEFAULT_NETWORK ->
                    "Network temporarily unavailable"
                else -> "Checking network"
            }
}
