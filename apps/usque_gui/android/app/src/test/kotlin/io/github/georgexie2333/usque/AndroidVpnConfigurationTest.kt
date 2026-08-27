package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress

class AndroidVpnConfigurationTest {
    @Test
    fun ipv4OnlyEndpointPolicyStillBuildsADualStackTunnel() {
        val profile = profile("ipv4Only")

        assertTrue(profile.includeIpv4)
        assertTrue(profile.includeIpv6)
        assertEquals(listOf(profile.dnsIpv4, profile.dnsIpv6), profile.dnsServers)
    }

    @Test
    fun ipv6OnlyEndpointPolicyStillBuildsADualStackTunnel() {
        val profile = profile("ipv6Only")

        assertTrue(profile.includeIpv4)
        assertTrue(profile.includeIpv6)
        assertEquals(2, profile.dnsServers.size)
    }

    @Test
    fun geoCountriesEnableSplitDnsWithoutReplacingWarpUpstreams() {
        val profile = profile("auto").copy(geoDirectCountries = listOf("CN"))

        assertTrue(profile.splitDnsEnabled)
        assertEquals(listOf(profile.dnsIpv4, profile.dnsIpv6), profile.dnsServers)
    }

    private fun profile(ipPolicy: String): AndroidVpnProfile =
        AndroidVpnProfile(
            id = "11111111-2222-4333-8444-555555555555",
            name = "Endpoint policy test",
            ipPolicy = ipPolicy,
            mtu = 1280,
            dnsMode = "tunnel",
            dnsIpv4 = InetAddress.getByName("1.1.1.1") as Inet4Address,
            dnsIpv6 = InetAddress.getByName("2606:4700:4700::1111") as Inet6Address,
            killSwitch = true,
            allowLan = false,
            bypassCidrs = emptyList(),
        )
}
