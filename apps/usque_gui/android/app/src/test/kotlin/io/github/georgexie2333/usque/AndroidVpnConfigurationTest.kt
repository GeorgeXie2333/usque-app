package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONException
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
    fun onlyEnabledHttpAndSocksChainsCaptureBeforeWaitingForTheNetwork() {
        for (source in listOf("http_proxy", "socks5_proxy", "wireguard_custom", "openvpn_custom", "vpn_gate")) {
            for (enabled in listOf(false, true)) {
                val json =
                    jsonProfile()
                        .put("chain_exit", JSONObject().put("enabled", enabled).put("source", source))
                        .put("vpn_gate", JSONObject().put("enabled", true))
                val profile = AndroidVpnProfile.parse(json.toString())
                assertEquals(enabled && source in setOf("http_proxy", "socks5_proxy"), profile.proxyChainEnabled)
                assertEquals(enabled, profile.vpnGateEnabled)
            }
        }
        assertEquals(false, AndroidVpnProfile.parse(jsonProfile().toString()).proxyChainEnabled)
    }

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
    fun automaticIgnoresDormantAddressesInOrganisationRanges() {
        val source =
            jsonProfile()
                .put("endpoint_selection", "automatic")
                .put("endpoint_v4", "162.159.197.2")
                .put("dns_v4", "162.159.197.2")
        assertEquals("162.159.197.2", AndroidVpnProfile.parse(source.toString()).dnsIpv4.hostAddress)
        source
            .put("endpoint_v4", "162.159.198.2")
            .put("dns_v4", "1.1.1.1")
            .put("endpoint_v6", "2606:4700:102::2")
            .put("dns_v6", "2606:4700:102::2")
        assertEquals(
            InetAddress.getByName("2606:4700:102::2"),
            AndroidVpnProfile.parse(source.toString()).dnsIpv6,
        )
        source.put("endpoint_selection", "custom")
        assertThrows(IllegalArgumentException::class.java) { AndroidVpnProfile.parse(source.toString()) }
    }

    @Test
    fun customAcceptsAddressesWithoutAnEndpointPoolRestriction() {
        for ((ipv4, ipv6) in listOf(
            "162.159.197.2" to "2606:4700:102::2",
            "192.0.2.45" to "2001:db8::45",
            "10.20.30.40" to "fd00::40",
        )) {
            val source =
                jsonProfile()
                    .put("endpoint_selection", "custom")
                    .put("endpoint_v4", ipv4)
                    .put("endpoint_v6", ipv6)
            assertEquals(source.getString("id"), AndroidVpnProfile.parse(source.toString()).id)
        }
    }

    @Test
    fun customDomainsAcceptDnsLengthBoundariesWithoutTheCidrLimit() {
        for (length in listOf(128, 129, 253)) {
            val domain = domainOfLength(length)
            val source = jsonProfile().put("bypass_domains", JSONArray().put(domain))
            assertEquals(listOf(domain), AndroidVpnProfile.parse(source.toString()).bypassDomains)
        }
        val source = jsonProfile().put("bypass_domains", JSONArray().put(domainOfLength(254)))
        val error = assertThrows(IllegalArgumentException::class.java) { AndroidVpnProfile.parse(source.toString()) }
        assertEquals("Invalid bypass domain", error.message)
    }

    @Test
    fun customDomainsKeepTrailingDotsAndUnicodeForCoreNormalization() {
        val supplementary = List(4) { "\uD840\uDC00".repeat(32) }.joinToString(".")
        assertTrue(supplementary.length > 253)
        for (domain in listOf(domainOfLength(253) + ".", "BÜCHER.example.", "xn--bcher-kva.example", supplementary)) {
            val source = jsonProfile().put("bypass_domains", JSONArray().put(" $domain "))
            assertEquals(listOf(domain), AndroidVpnProfile.parse(source.toString()).bypassDomains)
        }
    }

    @Test
    fun absentNullAndEmptyCustomDomainsKeepSplitDnsDisabled() {
        for (source in listOf(
            jsonProfile(),
            jsonProfile().put("bypass_domains", JSONObject.NULL),
            jsonProfile().put("bypass_domains", JSONArray()),
        )) {
            val profile = AndroidVpnProfile.parse(source.toString())
            assertTrue(profile.bypassDomains.isEmpty())
            assertEquals(false, profile.splitDnsEnabled)
            assertEquals(false, profile.requiresPhysicalDns)
        }
    }

    @Test
    fun emptyAndNullCustomDomainEntriesAreRejected() {
        for (domain in listOf("", " \t ", ".")) {
            val source = jsonProfile().put("bypass_domains", JSONArray().put(domain))
            val error =
                assertThrows(IllegalArgumentException::class.java) { AndroidVpnProfile.parse(source.toString()) }
            assertEquals("Invalid bypass domain", error.message)
        }
        val source = jsonProfile().put("bypass_domains", JSONArray().put(JSONObject.NULL))
        assertThrows(JSONException::class.java) { AndroidVpnProfile.parse(source.toString()) }
    }

    @Test
    fun customDomainLengthDoesNotRelaxBypassCidrParsing() {
        val cidrs = listOf("192.0.2.0/24", "2001:db8::/32")
        val source =
            jsonProfile()
                .put("bypass_domains", JSONArray().put(domainOfLength(253)))
                .put("bypass_cidrs", JSONArray(cidrs))
        assertEquals(cidrs, AndroidVpnProfile.parse(source.toString()).bypassCidrs)
        source.put("bypass_cidrs", JSONArray().put("a".repeat(129)))
        val error = assertThrows(IllegalArgumentException::class.java) { AndroidVpnProfile.parse(source.toString()) }
        assertEquals("Invalid bypass CIDR", error.message)
    }

    @Test
    fun customDomainsEnableSplitDnsAndInvalidateTunIdentityWithoutCountries() {
        val before = AndroidVpnProfile.parse(jsonProfile().toString())
        val after =
            AndroidVpnProfile.parse(
                jsonProfile().put("bypass_domains", JSONArray().put(domainOfLength(253))).toString(),
            )
        assertTrue(after.geoDirectCountries.isEmpty())
        assertTrue(after.splitDnsEnabled)
        assertTrue(after.requiresPhysicalDns)
        for (mode in listOf("doh", "dot")) {
            assertEquals(false, after.copy(directDnsMode = mode).requiresPhysicalDns)
        }
        assertTrue(!TunIdentity.from(before).sameForReuse(TunIdentity.from(after)))
        val shorter = after.copy(bypassDomains = listOf("example.com"))
        assertTrue(TunIdentity.from(shorter).sameForReuse(TunIdentity.from(after)))
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

    private fun domainOfLength(length: Int): String {
        val labels = mutableListOf<String>()
        var remaining = length
        while (remaining > 63) {
            val labelLength = minOf(63, remaining - 2)
            labels += "a".repeat(labelLength)
            remaining -= labelLength + 1
        }
        labels += "a".repeat(remaining)
        return labels.joinToString(".")
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
