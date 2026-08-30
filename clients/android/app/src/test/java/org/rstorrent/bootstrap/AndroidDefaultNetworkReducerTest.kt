package org.rstorrent.bootstrap

import org.junit.Assert.assertEquals
import org.junit.Test

class AndroidDefaultNetworkReducerTest {
    @Test
    fun disabledPreferenceIsAlwaysUnrestricted() {
        var state = state(enabled = false, blockedRequired = true)
        assertEquals(AndroidNetworkEligibility.UNRESTRICTED, state.eligibility)
        state = reduce(state, AndroidDefaultNetworkFact.Available(1))
        state = reduce(state, capabilities(1, notMetered = false))
        state = reduce(state, AndroidDefaultNetworkFact.Blocked(1, true))
        assertEquals(AndroidNetworkEligibility.UNRESTRICTED, state.eligibility)
    }

    @Test
    fun enabledPreferenceFailsClosedUntilOrderedFactsArrive() {
        var state = state(enabled = true, blockedRequired = true)
        assertEquals(AndroidNetworkEligibility.WAITING_FOR_DEFAULT_NETWORK, state.eligibility)
        state = reduce(state, AndroidDefaultNetworkFact.Available(1))
        assertEquals(AndroidNetworkEligibility.WAITING_FOR_CAPABILITIES, state.eligibility)
        state = reduce(state, capabilities(1))
        assertEquals(AndroidNetworkEligibility.WAITING_FOR_USABLE_NETWORK, state.eligibility)
        state = reduce(state, AndroidDefaultNetworkFact.Blocked(1, false))
        assertEquals(AndroidNetworkEligibility.UNRESTRICTED, state.eligibility)
    }

    @Test
    fun api28DoesNotRequireBlockedFact() {
        var state = state(enabled = true, blockedRequired = false)
        state = reduce(state, AndroidDefaultNetworkFact.Available(1))
        state = reduce(state, capabilities(1))
        assertEquals(AndroidNetworkEligibility.UNRESTRICTED, state.eligibility)
    }

    @Test
    fun capabilityReasonsAreClosedAndSpecific() {
        var state = state(enabled = true, blockedRequired = false)
        state = reduce(state, AndroidDefaultNetworkFact.Available(1))
        state = reduce(state, capabilities(1, internet = false))
        assertEquals(AndroidNetworkEligibility.WAITING_FOR_VALIDATED_INTERNET, state.eligibility)
        state = reduce(state, capabilities(1, validated = false))
        assertEquals(AndroidNetworkEligibility.WAITING_FOR_VALIDATED_INTERNET, state.eligibility)
        state = reduce(state, capabilities(1, notMetered = false))
        assertEquals(AndroidNetworkEligibility.WAITING_FOR_UNMETERED_NETWORK, state.eligibility)
        state = reduce(state, capabilities(1, notSuspended = false))
        assertEquals(AndroidNetworkEligibility.WAITING_FOR_USABLE_NETWORK, state.eligibility)
    }

    @Test
    fun currentNetworkReplacementRejectsStaleFacts() {
        var state = state(enabled = true, blockedRequired = true)
        state = reduce(state, AndroidDefaultNetworkFact.Available(1))
        state = reduce(state, capabilities(1))
        state = reduce(state, AndroidDefaultNetworkFact.Blocked(1, false))
        state = reduce(state, AndroidDefaultNetworkFact.Available(2))
        val replacementRevision = state.revision

        state = reduce(state, capabilities(1, notMetered = false))
        state = reduce(state, AndroidDefaultNetworkFact.Blocked(1, false))
        state = reduce(state, AndroidDefaultNetworkFact.Lost(1))

        assertEquals(replacementRevision, state.revision)
        assertEquals(2L, state.currentNetworkToken)
        assertEquals(AndroidNetworkEligibility.WAITING_FOR_CAPABILITIES, state.eligibility)
    }

    @Test
    fun lossAndPreferenceChangesPreserveConservativeState() {
        var state = state(enabled = true, blockedRequired = false)
        state = reduce(state, AndroidDefaultNetworkFact.Available(1))
        state = reduce(state, capabilities(1))
        assertEquals(AndroidNetworkEligibility.UNRESTRICTED, state.eligibility)
        state = reduce(state, AndroidDefaultNetworkFact.Lost(1))
        assertEquals(AndroidNetworkEligibility.WAITING_FOR_DEFAULT_NETWORK, state.eligibility)
        state = reduce(state, AndroidDefaultNetworkFact.Preference(false))
        assertEquals(AndroidNetworkEligibility.UNRESTRICTED, state.eligibility)
        state = reduce(state, AndroidDefaultNetworkFact.Preference(true))
        assertEquals(AndroidNetworkEligibility.WAITING_FOR_DEFAULT_NETWORK, state.eligibility)
    }

    @Test
    fun duplicateFactsDoNotAdvanceRevision() {
        var state = state(enabled = true, blockedRequired = false)
        state = reduce(state, AndroidDefaultNetworkFact.Available(1))
        state = reduce(state, capabilities(1))
        val revision = state.revision
        state = reduce(state, capabilities(1))
        assertEquals(revision, state.revision)
    }

    private fun state(
        enabled: Boolean,
        blockedRequired: Boolean,
    ) =
        AndroidDefaultNetworkState(
            unmeteredNetworksOnly = enabled,
            blockedFactRequired = blockedRequired,
        )

    private fun reduce(
        state: AndroidDefaultNetworkState,
        fact: AndroidDefaultNetworkFact,
    ) = AndroidDefaultNetworkReducer.reduce(state, fact)

    private fun capabilities(
        token: Long,
        internet: Boolean = true,
        validated: Boolean = true,
        notMetered: Boolean = true,
        notSuspended: Boolean = true,
    ) =
        AndroidDefaultNetworkFact.Capabilities(
            token,
            AndroidNetworkCapabilityFacts(internet, validated, notMetered, notSuspended),
        )
}
