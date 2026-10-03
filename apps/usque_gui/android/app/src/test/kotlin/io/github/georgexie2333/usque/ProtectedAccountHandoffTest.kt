package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicLong

class ProtectedAccountHandoffTest {
    private fun profile(
        source: String = "http_proxy",
        enabled: Boolean = true,
        tunnel: Boolean = true,
        killSwitch: Boolean = true,
    ): String =
        JSONObject()
            .put("id", "a")
            .put("frontends", JSONObject().put("tunnel", tunnel))
            .put("kill_switch", killSwitch)
            .put("chain_exit", JSONObject().put("source", source).put("enabled", enabled))
            .toString()

    @Test
    fun liveAdmissionRequiresEstablishedProxyVpnAndOwnedInterfaceRegardlessOfKillSwitch() {
        for (source in listOf("http_proxy", "socks5_proxy", "wireguard_custom", "openvpn_custom", "vpn_gate")) {
            for (killSwitch in listOf(false, true)) {
                val token = ProtectedAccountHandoff().request(profile(source, killSwitch = killSwitch), true, "vpn")
                if (source in setOf("http_proxy", "socks5_proxy")) assertNotNull(token) else assertNull(token)
            }
        }
        for (applied in listOf(null, "{}", profile(enabled = false), profile(tunnel = false))) {
            assertNull(ProtectedAccountHandoff().request(applied, true, "vpn"))
        }
        assertNull(ProtectedAccountHandoff().request(profile(), false, "vpn"))
        assertNull(ProtectedAccountHandoff().request(profile(), true, "socks5"))
        assertNotNull(ProtectedAccountHandoff().request(profile(), true, "vpn", phase = "preparing"))
        assertNull(
            ProtectedAccountHandoff().request(profile(), true, "vpn", runtimeProfile = profile("wireguard_custom")),
        )
        assertNull(ProtectedAccountHandoff().request(profile(), true, "vpn", runtimeProfile = "{\"id\":\"b\"}"))
    }

    @Test
    fun latestAccountWinsAndLateReadCannotBeginAnOlderReplacement() {
        val handoff = ProtectedAccountHandoff()
        val toB = handoff.request(profile(), true, "vpn")!!
        val toC = handoff.request(profile(), true, "vpn")!!
        assertFalse(handoff.begin(toB))
        assertTrue(handoff.begin(toC))
        assertTrue(handoff.retained)
        assertFalse(handoff.owns(toB))
    }

    @Test
    fun sourceAndTargetManualPreferencesDetermineTerminalRetention() {
        for (sourceKillSwitch in listOf(false, true)) {
            for (targetKillSwitch in listOf(false, true)) {
                val handoff = ProtectedAccountHandoff()
                assertTrue(handoff.begin(handoff.request(profile(killSwitch = sourceKillSwitch), true, "vpn")!!))
                assertTrue(handoff.retained)
                val retainsFailure = handoff.retainAfterFailure(targetKillSwitch)
                assertTrue(retainsFailure == (sourceKillSwitch || targetKillSwitch))
                var released = false
                ProxyChainVpnLifecycle.releaseAfterStop(true, retainsFailure, false, { true }) { released = true }
                assertFalse(released)
                ProxyChainVpnLifecycle.releaseAfterStop(true, retainsFailure, true, { true }) { released = true }
                assertTrue(released == (!sourceKillSwitch && !targetKillSwitch))
            }
        }
    }

    @Test
    fun rapidOffTargetsCannotEraseInheritedSourceOnPreference() {
        val handoff = ProtectedAccountHandoff()
        assertTrue(handoff.begin(handoff.request(profile(), true, "vpn")!!))
        val toC = handoff.request(profile(killSwitch = false), true, "socks5")!!
        assertTrue(handoff.begin(toC))
        assertTrue(handoff.retainAfterFailure(false))
        handoff.stable()
        assertFalse(handoff.retained)
        assertFalse(handoff.retainAfterFailure(false))
        assertNotNull(handoff.request(profile(killSwitch = false), true, "vpn"))
    }

    @Test
    fun temporaryCaptureDoesNotInventKillSwitchForRapidOffTargets() {
        val handoff = ProtectedAccountHandoff()
        val off = profile(killSwitch = false)
        assertTrue(handoff.begin(handoff.request(off, true, "vpn")!!))
        assertTrue(handoff.retained)
        assertTrue(handoff.begin(handoff.request(off, true, "socks5")!!))
        assertFalse(handoff.retainAfterFailure(false))
        assertTrue(handoff.retainAfterFailure(true))
    }

    @Test
    fun unknownSourcePreferenceDuringHandoffRetainsConservatively() {
        val handoff = ProtectedAccountHandoff()
        assertTrue(handoff.begin(handoff.request(profile(killSwitch = false), true, "vpn")!!))
        assertTrue(handoff.begin(handoff.request(null, true, "socks5")!!))
        assertTrue(handoff.retainAfterFailure(false))
    }

    @Test
    fun explicitDisconnectRetiresPendingCallbacksAndInheritance() {
        val handoff = ProtectedAccountHandoff()
        val token = handoff.request(profile(), true, "vpn")!!
        assertTrue(handoff.begin(token))
        handoff.disconnect()
        assertFalse(handoff.retained)
        assertFalse(handoff.retainAfterFailure(false))
        assertFalse(handoff.begin(token))
        assertNull(handoff.request(profile(), false, "vpn"))
    }

    @Test
    fun admittingCBeforeItsCatalogReadPreventsBFromReleasingTheInheritedTun() {
        val handoff = ProtectedAccountHandoff()
        assertTrue(handoff.begin(handoff.request(profile(), true, "vpn")!!))
        val generation = AtomicLong(1)
        val bGeneration = generation.get()
        var released = false
        val delayedBSuccess = {
            if (generation.get() == bGeneration) {
                released = true
                handoff.stable()
            }
        }
        val toC = handoff.request(profile(killSwitch = false), true, "socks5")!!
        assertTrue(handoff.begin(toC))
        generation.incrementAndGet()
        delayedBSuccess()
        assertFalse(released)
        assertTrue(handoff.retained)
    }

    @Test
    fun coldSettingsReplacementPreservesAppliedSourceUntilTheTargetIsStable() {
        for (sourceKillSwitch in listOf(false, true)) {
            val handoff = ProtectedAccountHandoff()
            val source = profile(killSwitch = sourceKillSwitch)
            assertTrue(handoff.inheritColdVpn(source, tunnelOwned = true, targetTunnel = true))
            assertTrue(handoff.retained)
            var released = false
            ProxyChainVpnLifecycle.releaseAfterStop(
                proxyChain = true,
                killSwitch = handoff.retainAfterFailure(false),
                confirmed = true,
                isCurrent = { true },
                releaseOwnedTun = { released = true },
            )
            assertTrue(released == !sourceKillSwitch)
            handoff.stable()
            assertFalse(handoff.retainAfterFailure(false))
        }
    }

    @Test
    fun coldVpnRemovalOrUnownedSavedProfileDoesNotAcquireProtection() {
        val handoff = ProtectedAccountHandoff()
        assertFalse(handoff.inheritColdVpn(profile(), tunnelOwned = true, targetTunnel = false))
        assertFalse(handoff.inheritColdVpn(profile(), tunnelOwned = false, targetTunnel = true))
        assertFalse(handoff.inheritColdVpn(null, tunnelOwned = true, targetTunnel = true))
        assertFalse(handoff.retained)
    }
}
