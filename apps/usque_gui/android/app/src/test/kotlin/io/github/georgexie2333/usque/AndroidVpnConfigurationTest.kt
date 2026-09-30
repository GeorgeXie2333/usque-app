package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress

class AndroidVpnConfigurationTest {
    @Test
    fun automaticIgnoresDormantCustomDnsClashesButLegacyCustomRemainsStrict() {
        val source = jsonProfile().put("endpoint_v4", "1.1.1.1")
        assertThrows(IllegalArgumentException::class.java) { AndroidVpnProfile.parse(source.toString()) }
        source.put("endpoint_selection", "custom")
        assertThrows(IllegalArgumentException::class.java) { AndroidVpnProfile.parse(source.toString()) }
        source.put("endpoint_selection", "automatic")
        assertEquals("1.1.1.1", AndroidVpnProfile.parse(source.toString()).dnsIpv4.hostAddress)
        source.put("endpoint_selection", "unknown")
        assertThrows(IllegalArgumentException::class.java) { AndroidVpnProfile.parse(source.toString()) }
    }

    @Test
    fun managedEndpointDnsClashesRemainRejectedInAutomaticMode() {
        val source =
            jsonProfile()
                .put("endpoint_selection", "automatic")
                .put("endpoint_v4", "162.159.197.2")
                .put("dns_v4", "162.159.197.2")
        assertThrows(IllegalArgumentException::class.java) { AndroidVpnProfile.parse(source.toString()) }
        source
            .put("endpoint_v4", "162.159.198.2")
            .put("dns_v4", "1.1.1.1")
            .put("endpoint_v6", "2606:4700:102::2")
            .put("dns_v6", "2606:4700:102::2")
        assertThrows(IllegalArgumentException::class.java) { AndroidVpnProfile.parse(source.toString()) }
    }

    @Test
    fun customDomainsEnableSplitDnsAndInvalidateTunIdentityWithoutCountries() {
        val before = profile("automatic")
        val after = before.copy(bypassDomains = listOf("example.com"))
        assertTrue(after.geoDirectCountries.isEmpty())
        assertTrue(after.splitDnsEnabled)
        assertTrue(after.requiresPhysicalDns)
        assertTrue(!after.copy(directDnsMode = "doh").requiresPhysicalDns)
        assertTrue(!TunIdentity.from(before).sameForReuse(TunIdentity.from(after)))
    }

    @Test
    fun customExitKeepsSyntheticDnsEvenWithoutUsableUpstreams() {
        val configured = profile("automatic").copy(vpnGateEnabled = true, customChain = true, dnsMode = "system")
        assertTrue(configured.splitDnsEnabled)
        val network =
            VpnGateNetwork.parse(
                JSONObject()
                    .put(
                        "ipv4",
                        "10.8.0.2",
                    ).put("ipv6", JSONObject.NULL)
                    .put("mtu", 1280)
                    .put("dns_servers", JSONArray()),
            )
        assertTrue(network.dns.isEmpty())
        assertTrue(configured.dnsServers.isNotEmpty())
    }

    @Test
    fun gateRemoteDnsUsesInternalRoutesAndPreservesExplicitLocalDns() {
        val gate = profile("automatic").copy(vpnGateEnabled = true, allowLan = true)
        assertTrue(gate.splitDnsEnabled)
        assertEquals(false, gate.requiresPhysicalDns)
        assertEquals(false, gate.copy(dnsMode = "localConfigured").splitDnsEnabled)
        assertTrue(gate.copy(dnsMode = "localConfigured", geoDirectCountries = listOf("CN")).splitDnsEnabled)
    }

    @Test
    fun l4AlwaysAddsInternalDnsWithoutRequiringPhysicalDnsOrGeo() {
        val l4 = profile("ipv4Only").copy(dataPlane = "l4_proxy")
        assertTrue(l4.splitDnsEnabled)
        assertEquals(false, l4.requiresPhysicalDns)
        assertEquals(listOf(l4.dnsIpv4, l4.dnsIpv6), l4.dnsServers)
    }

    @Test
    fun encryptedBootstrapDoesNotRequirePhysicalDnsMetadata() {
        val geo = profile("automatic").copy(geoDirectCountries = listOf("CN"))
        assertTrue(geo.requiresPhysicalDns)
        for (mode in listOf("doh", "dot")) {
            assertEquals(false, geo.copy(directDnsMode = mode).requiresPhysicalDns)
            assertTrue(geo.copy(directDnsMode = mode).splitDnsEnabled)
        }
        assertEquals(false, profile("automatic").requiresPhysicalDns)
    }

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

    private fun jsonProfile(): JSONObject =
        JSONObject()
            .put("id", "11111111-2222-4333-8444-555555555555")
            .put("name", "Endpoint policy test")
            .put("mode", "vpn")
            .put("ip_policy", "automatic")
            .put("mtu", 1280)
            .put("dns_mode", "tunnel")
            .put("dns_v4", "1.1.1.1")
            .put("dns_v6", "2606:4700:4700::1111")
            .put("endpoint_v4", "162.159.198.2")
            .put("endpoint_v6", "2606:4700:103::2")
            .put("kill_switch", true)
            .put("allow_lan", false)
            .put("bypass_cidrs", JSONArray())

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
