package io.github.georgexie2333.usque

import java.net.InetAddress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class VpnRoutePlannerTest {
    @Test
    fun dnsAddressesCoveredByLanOrExplicitBypassAreDetected() {
        assertTrue(
            VpnRoutePlanner.isAddressExcluded(
                InetAddress.getByName("192.168.1.1"),
                allowLan = true,
                bypassCidrs = emptyList(),
            ),
        )
        assertTrue(
            VpnRoutePlanner.isAddressExcluded(
                InetAddress.getByName("1.1.1.1"),
                allowLan = false,
                bypassCidrs = listOf("1.1.1.0/24"),
            ),
        )
        assertFalse(
            VpnRoutePlanner.isAddressExcluded(
                InetAddress.getByName("1.1.1.1"),
                allowLan = false,
                bypassCidrs = emptyList(),
            ),
        )
    }

    @Test
    fun fullTunnelInstallsOneRoutePerEnabledFamily() {
        val plan =
            VpnRoutePlanner.plan(
                includeIpv4 = true,
                includeIpv6 = true,
                allowLan = false,
                bypassCidrs = emptyList(),
                supportsRouteExclusion = false,
            )

        assertEquals(listOf("0.0.0.0/0", "0:0:0:0:0:0:0:0/0"), plan.included.map { it.key })
        assertTrue(plan.excluded.isEmpty())
    }

    @Test
    fun androidThirteenUsesNativeRouteExclusions() {
        val plan =
            VpnRoutePlanner.plan(
                includeIpv4 = true,
                includeIpv6 = false,
                allowLan = false,
                bypassCidrs = listOf("192.0.2.25/24"),
                supportsRouteExclusion = true,
            )

        assertEquals(listOf("0.0.0.0/0"), plan.included.map { it.key })
        assertEquals(listOf("192.0.2.0/24"), plan.excluded.map { it.key })
    }

    @Test
    fun olderAndroidBuildsTheExactComplement() {
        val excluded = CidrBlock.parse("10.0.0.0/8")
        val plan =
            VpnRoutePlanner.plan(
                includeIpv4 = true,
                includeIpv6 = false,
                allowLan = false,
                bypassCidrs = listOf(excluded.key),
                supportsRouteExclusion = false,
            )

        assertEquals(8, plan.included.size)
        assertTrue(plan.included.all { route -> !route.contains(excluded) })
        assertFalse(plan.included.any { route -> route.key == "0.0.0.0/0" })
    }

    @Test
    fun wholeFamilyBypassIsRejected() {
        assertThrows(IllegalArgumentException::class.java) {
            VpnRoutePlanner.plan(
                includeIpv4 = true,
                includeIpv6 = false,
                allowLan = false,
                bypassCidrs = listOf("0.0.0.0/0"),
                supportsRouteExclusion = false,
            )
        }
    }
}
