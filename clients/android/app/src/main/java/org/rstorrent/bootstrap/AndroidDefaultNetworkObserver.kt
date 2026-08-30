package org.rstorrent.bootstrap

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.Build

enum class AndroidNetworkEligibility {
    UNRESTRICTED,
    WAITING_FOR_DEFAULT_NETWORK,
    WAITING_FOR_CAPABILITIES,
    WAITING_FOR_VALIDATED_INTERNET,
    WAITING_FOR_UNMETERED_NETWORK,
    WAITING_FOR_USABLE_NETWORK,
}

internal data class AndroidNetworkCapabilityFacts(
    val internet: Boolean,
    val validated: Boolean,
    val notMetered: Boolean,
    val notSuspended: Boolean,
)

internal sealed interface AndroidDefaultNetworkFact {
    data class Preference(val unmeteredNetworksOnly: Boolean) : AndroidDefaultNetworkFact

    data class Available(val networkToken: Long) : AndroidDefaultNetworkFact

    data class Capabilities(
        val networkToken: Long,
        val capabilities: AndroidNetworkCapabilityFacts,
    ) : AndroidDefaultNetworkFact

    data class Blocked(
        val networkToken: Long,
        val blocked: Boolean,
    ) : AndroidDefaultNetworkFact

    data class Lost(val networkToken: Long) : AndroidDefaultNetworkFact
}

internal data class AndroidDefaultNetworkState(
    val unmeteredNetworksOnly: Boolean,
    val blockedFactRequired: Boolean,
    val currentNetworkToken: Long? = null,
    val capabilities: AndroidNetworkCapabilityFacts? = null,
    val blocked: Boolean? = null,
    val revision: Long = 0,
) {
    val eligibility: AndroidNetworkEligibility
        get() {
            if (!unmeteredNetworksOnly) return AndroidNetworkEligibility.UNRESTRICTED
            if (currentNetworkToken == null) {
                return AndroidNetworkEligibility.WAITING_FOR_DEFAULT_NETWORK
            }
            val currentCapabilities = capabilities
                ?: return AndroidNetworkEligibility.WAITING_FOR_CAPABILITIES
            if (!currentCapabilities.internet || !currentCapabilities.validated) {
                return AndroidNetworkEligibility.WAITING_FOR_VALIDATED_INTERNET
            }
            if (!currentCapabilities.notMetered) {
                return AndroidNetworkEligibility.WAITING_FOR_UNMETERED_NETWORK
            }
            if (
                !currentCapabilities.notSuspended ||
                blocked == true ||
                (blockedFactRequired && blocked == null)
            ) {
                return AndroidNetworkEligibility.WAITING_FOR_USABLE_NETWORK
            }
            return AndroidNetworkEligibility.UNRESTRICTED
        }
}

internal object AndroidDefaultNetworkReducer {
    fun reduce(
        state: AndroidDefaultNetworkState,
        fact: AndroidDefaultNetworkFact,
    ): AndroidDefaultNetworkState {
        val next =
            when (fact) {
                is AndroidDefaultNetworkFact.Preference ->
                    if (fact.unmeteredNetworksOnly == state.unmeteredNetworksOnly) state
                    else state.copy(unmeteredNetworksOnly = fact.unmeteredNetworksOnly)
                is AndroidDefaultNetworkFact.Available ->
                    state.copy(
                        currentNetworkToken = fact.networkToken,
                        capabilities = null,
                        blocked = null,
                    )
                is AndroidDefaultNetworkFact.Capabilities ->
                    if (fact.networkToken != state.currentNetworkToken) state
                    else state.copy(capabilities = fact.capabilities)
                is AndroidDefaultNetworkFact.Blocked ->
                    if (fact.networkToken != state.currentNetworkToken) state
                    else state.copy(blocked = fact.blocked)
                is AndroidDefaultNetworkFact.Lost ->
                    if (fact.networkToken != state.currentNetworkToken) state
                    else state.copy(
                        currentNetworkToken = null,
                        capabilities = null,
                        blocked = null,
                    )
            }
        if (next == state) return state
        check(state.revision != Long.MAX_VALUE) { "network observation revision exhausted" }
        return next.copy(revision = state.revision + 1)
    }
}

internal class AndroidDefaultNetworkObserver(
    context: Context,
    unmeteredNetworksOnly: Boolean,
    private val onState: (AndroidDefaultNetworkState) -> Unit,
) : AutoCloseable {
    private val connectivityManager = context.getSystemService(ConnectivityManager::class.java)
    private val ownership = Any()
    private var state =
        AndroidDefaultNetworkState(
            unmeteredNetworksOnly = unmeteredNetworksOnly,
            blockedFactRequired = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q,
        )
    private var registered = false

    private val callback =
        object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                apply(AndroidDefaultNetworkFact.Available(network.networkHandle))
            }

            override fun onCapabilitiesChanged(
                network: Network,
                capabilities: NetworkCapabilities,
            ) {
                apply(
                    AndroidDefaultNetworkFact.Capabilities(
                        network.networkHandle,
                        AndroidNetworkCapabilityFacts(
                            internet =
                                capabilities.hasCapability(
                                    NetworkCapabilities.NET_CAPABILITY_INTERNET,
                                ),
                            validated =
                                capabilities.hasCapability(
                                    NetworkCapabilities.NET_CAPABILITY_VALIDATED,
                                ),
                            notMetered =
                                capabilities.hasCapability(
                                    NetworkCapabilities.NET_CAPABILITY_NOT_METERED,
                                ),
                            notSuspended =
                                capabilities.hasCapability(
                                    NetworkCapabilities.NET_CAPABILITY_NOT_SUSPENDED,
                                ),
                        ),
                    ),
                )
            }

            override fun onBlockedStatusChanged(
                network: Network,
                blocked: Boolean,
            ) {
                apply(AndroidDefaultNetworkFact.Blocked(network.networkHandle, blocked))
            }

            override fun onLost(network: Network) {
                apply(AndroidDefaultNetworkFact.Lost(network.networkHandle))
            }
        }

    fun start(): Boolean {
        synchronized(ownership) {
            check(!registered) { "default-network callback is already registered" }
            return try {
                connectivityManager.registerDefaultNetworkCallback(callback)
                registered = true
                true
            } catch (_: RuntimeException) {
                false
            }
        }
    }

    fun setUnmeteredNetworksOnly(enabled: Boolean): AndroidDefaultNetworkState =
        apply(AndroidDefaultNetworkFact.Preference(enabled))

    fun snapshot(): AndroidDefaultNetworkState = synchronized(ownership) { state }

    fun isRegistered(): Boolean = synchronized(ownership) { registered }

    override fun close() {
        synchronized(ownership) {
            if (!registered) return
            connectivityManager.unregisterNetworkCallback(callback)
            registered = false
        }
    }

    private fun apply(fact: AndroidDefaultNetworkFact): AndroidDefaultNetworkState {
        val updated: AndroidDefaultNetworkState
        val changed: Boolean
        synchronized(ownership) {
            updated = AndroidDefaultNetworkReducer.reduce(state, fact)
            changed = updated != state
            state = updated
        }
        if (changed) onState(updated)
        return updated
    }
}
