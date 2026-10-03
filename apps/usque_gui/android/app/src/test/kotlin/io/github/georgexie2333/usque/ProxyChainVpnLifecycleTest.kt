package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicReference

class ProxyChainVpnLifecycleTest {
    @Test
    fun inheritedProxyOnlyTargetsUseManualFailurePolicyWithoutVpnParsing() {
        val proxyOnly = """{"frontends":{"tunnel":false},"kill_switch":false}"""
        for (sourceKillSwitch in listOf(false, true)) {
            val policy = ProxyChainVpnLifecycle.failurePolicy(proxyOnly, true, sourceKillSwitch)
            assertTrue(policy.protectedCapture)
            assertEquals(sourceKillSwitch, policy.killSwitch)
            var released = false
            ProxyChainVpnLifecycle.releaseAfterStop(policy.protectedCapture, policy.killSwitch, true, { true }) {
                released = true
            }
            assertEquals(!sourceKillSwitch, released)
        }
    }

    @Test
    fun unknownTargetPreferenceNeverReleasesInheritedCapture() {
        for (profile in listOf(null, "{invalid", "{}", """{"kill_switch":42}""")) {
            val policy = ProxyChainVpnLifecycle.failurePolicy(profile, true, false)
            assertTrue(policy.protectedCapture)
            assertTrue(policy.killSwitch)
        }
        val ordinary = ProxyChainVpnLifecycle.failurePolicy("""{"kill_switch":false}""", false, false)
        assertFalse(ordinary.protectedCapture)
    }

    @Test
    fun proxyCapturesBeforeWaitingEvenWithoutAPhysicalNetwork() {
        val calls = mutableListOf<String>()
        val result =
            ProxyChainVpnLifecycle.prepare(
                proxyChain = true,
                isCurrent = { true },
                awaitPhysicalNetwork = {
                    calls.add("wait")
                    false
                },
                establishTun = {
                    calls.add("tun")
                    Any()
                },
            )
        assertEquals(listOf("tun", "wait"), calls)
        assertSame(ProxyChainVpnLifecycle.Startup.WaitingForNetwork, result)
    }

    @Test
    fun nonProxyStartupStillWaitsBeforeCreatingAnyInterface() {
        var established = false
        val result =
            ProxyChainVpnLifecycle.prepare(
                proxyChain = false,
                isCurrent = { true },
                awaitPhysicalNetwork = { false },
                establishTun = {
                    established = true
                    Any()
                },
            )
        assertFalse(established)
        assertSame(ProxyChainVpnLifecycle.Startup.WaitingForNetwork, result)
    }

    @Test
    fun replacementIsPublishedBeforeWaitingAndCancellationCannotStartNative() {
        val old = Any()
        val replacement = Any()
        val owned = AtomicReference(old)
        var current = true
        val result =
            ProxyChainVpnLifecycle.prepare(
                proxyChain = true,
                isCurrent = { current },
                establishTun = {
                    // Models ensureTun's NEWFIRST ownership transfer.
                    assertSame(old, owned.getAndSet(replacement))
                    replacement
                },
                awaitPhysicalNetwork = {
                    assertSame(replacement, owned.get())
                    current = false
                    true
                },
            )
        assertSame(ProxyChainVpnLifecycle.Startup.Cancelled, result)
        assertSame(replacement, owned.get())
    }

    @Test
    fun cancelledGenerationCannotEstablishOrWait() {
        val result =
            ProxyChainVpnLifecycle.prepare<Any>(
                proxyChain = true,
                isCurrent = { false },
                awaitPhysicalNetwork = { error("stale wait") },
                establishTun = { error("stale interface") },
            )
        assertSame(ProxyChainVpnLifecycle.Startup.Cancelled, result)
    }

    @Test
    fun failedInterfaceCannotOpenPhysicalNetworkWait() {
        val result =
            ProxyChainVpnLifecycle.prepare<Any>(
                proxyChain = true,
                isCurrent = { true },
                awaitPhysicalNetwork = { error("interface was not created") },
                establishTun = { null },
            )
        assertSame(ProxyChainVpnLifecycle.Startup.TunUnavailable, result)
    }

    @Test
    fun terminalCleanupHonorsKillSwitchAndConfirmationWithoutClosingANewOwner() {
        for (proxy in listOf(false, true)) {
            for (killSwitch in listOf(false, true)) {
                for (confirmed in listOf(false, true)) {
                    for (current in listOf(false, true)) {
                        val old = Any()
                        val owned = AtomicReference<Any?>(old)
                        ProxyChainVpnLifecycle.releaseAfterStop(proxy, killSwitch, confirmed, { current }) {
                            owned.compareAndSet(old, null)
                        }
                        assertEquals(proxy && !killSwitch && confirmed && current, owned.get() == null)
                    }
                }
            }
        }
        val old = Any()
        val replacement = Any()
        val owned = AtomicReference<Any?>(replacement)
        ProxyChainVpnLifecycle.releaseAfterStop(true, false, true, { true }) {
            assertFalse(owned.compareAndSet(old, null))
        }
        assertSame(replacement, owned.get())
    }

    @Test
    fun successfulStartupHandsBackTheCapturedDescriptor() {
        val descriptor = Any()
        val result = ProxyChainVpnLifecycle.prepare(true, { true }, { true }, { descriptor })
        assertTrue(result is ProxyChainVpnLifecycle.Startup.Ready)
        assertSame(descriptor, (result as ProxyChainVpnLifecycle.Startup.Ready<*>).descriptor)
    }
}
