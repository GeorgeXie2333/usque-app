package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class EstablishedTunProtectionTest {
    private fun profile(
        id: String = "a",
        source: String = "http_proxy",
        killSwitch: Boolean = true,
    ): String =
        JSONObject()
            .put("id", id)
            .put("frontends", JSONObject().put("tunnel", true))
            .put("kill_switch", killSwitch)
            .put("chain_exit", JSONObject().put("enabled", true).put("source", source))
            .toString()

    @Test
    fun establishedEarlyTunAdmitsSelectionBeforeAnyConnectedSettingsConfirmation() {
        for (source in listOf("http_proxy", "socks5_proxy")) {
            for (phase in listOf("preparing", "connectingH3", "connectingH2", "error")) {
                val owned = Any()
                val applied = profile(source = source)
                val proof = EstablishedTunProtection()
                proof.established(owned, applied)
                val handoff = ProtectedAccountHandoff()
                assertNotNull(
                    handoff.request(
                        appliedProfile = proof.profileFor(owned),
                        tunnelOwned = true,
                        mode = "vpn",
                        runtimeProfile = applied,
                        phase = phase,
                    ),
                )
            }
        }
    }

    @Test
    fun savedOrPreviousProfileCannotSubstituteForAnEstablishedOwnedDescriptor() {
        val owned = Any()
        val stale = Any()
        val requested = profile()
        val proof = EstablishedTunProtection()
        val handoff = ProtectedAccountHandoff()
        assertNull(handoff.request(proof.profileFor(owned), true, "vpn", requested, "preparing"))
        proof.established(stale, requested)
        assertNull(handoff.request(proof.profileFor(owned), true, "vpn", requested, "error"))
        proof.closed(stale)
        assertNull(proof.profileFor(stale))
        assertNull(proof.profileFor(null))
    }

    @Test
    fun lateOldDescriptorClosureCannotEraseTheReplacementProfile() {
        val old = Any()
        val current = Any()
        val proof = EstablishedTunProtection()
        proof.established(old, profile("a"))
        val applied = profile("b")
        proof.established(current, applied)
        proof.closed(old)
        assertNull(proof.profileFor(old))
        assertEquals(applied, proof.profileFor(current))
        proof.closed(current)
        assertNull(proof.profileFor(current))
    }

    @Test
    fun pendingRulesNeverOverwriteEstablishedProtectionEligibility() {
        val owned = Any()
        val proof = EstablishedTunProtection()
        val handoff = ProtectedAccountHandoff()
        val pendingProxy = profile()
        for (applied in listOf(profile(source = "wireguard_custom"), profile("old"))) {
            proof.established(owned, applied)
            assertNull(handoff.request(proof.profileFor(owned), true, "vpn", pendingProxy, "error"))
        }
        proof.established(owned, pendingProxy)
        assertNull(
            handoff.request(proof.profileFor(owned), true, "vpn", profile(source = "wireguard_custom"), "preparing"),
        )
        assertNotNull(handoff.request(proof.profileFor(owned), true, "vpn", pendingProxy, "error"))
    }

    @Test
    fun retainedDescriptorKeepsItsAppliedProfileUntilNativeConfirmation() {
        val retained = Any()
        val replacement = Any()
        val applied = profile(killSwitch = true)
        val requested = profile(killSwitch = false)
        val proof = EstablishedTunProtection()
        proof.published(retained, null, applied)
        proof.published(retained, retained, requested)
        assertEquals(applied, proof.profileFor(retained))
        val handoff = ProtectedAccountHandoff()
        val token = handoff.request(proof.profileFor(retained), true, "vpn", requested, "preparing")!!
        handoff.begin(token)
        assertEquals(true, handoff.retainAfterFailure(false))
        // A successful native confirmation can update the same descriptor.
        proof.established(retained, requested)
        assertEquals(requested, proof.profileFor(retained))
        // A newly established descriptor carries its own requested scope.
        proof.published(replacement, retained, applied)
        assertEquals(applied, proof.profileFor(replacement))
        assertNull(proof.profileFor(retained))
    }
}
