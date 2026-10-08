package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress

internal data class AndroidVpnProfile(
    val id: String,
    val name: String,
    val ipPolicy: String,
    val mtu: Int,
    val dnsMode: String,
    val dnsIpv4: Inet4Address,
    val dnsIpv6: Inet6Address,
    val killSwitch: Boolean,
    val allowLan: Boolean,
    val bypassCidrs: List<String>,
    val geoDirectCountries: List<String> = emptyList(),
    val bypassDomains: List<String> = emptyList(),
    val directDnsMode: String = "physicalSystem",
    val dataPlane: String = "connect_ip",
    val vpnGateEnabled: Boolean = false,
    val customChain: Boolean = false,
    val proxyChainEnabled: Boolean = false,
    val warpDnsMode: String = "plain",
    val routingDomainRules: Boolean = false,
    val routingDirectDomains: Boolean = false,
) {
    // ipPolicy controls only the physical MASQUE endpoint. CONNECT-IP remains
    // dual-stack regardless of which outer address family carries it.
    val dnsServers: List<InetAddress>
        get() = listOf(dnsIpv4, dnsIpv6)

    val splitDnsEnabled: Boolean
        get() =
            (geoDirectCountries.isNotEmpty() || bypassDomains.isNotEmpty() || routingDomainRules) ||
                (vpnGateEnabled && (dnsMode == "tunnel" || customChain)) ||
                (dataPlane == "l4_proxy" && !vpnGateEnabled) ||
                (warpDnsMode != "plain" && !vpnGateEnabled)

    val requiresPhysicalDns: Boolean
        get() =
            (geoDirectCountries.isNotEmpty() || bypassDomains.isNotEmpty() || routingDirectDomains) &&
                directDnsMode == "physicalSystem"

    companion object {
        private val profileIdPattern =
            Regex(
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-" +
                    "[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
            )

        fun parse(profileJson: String): AndroidVpnProfile {
            require(profileJson.toByteArray(Charsets.UTF_8).size <= 256 * 1024) {
                "Android profile exceeds the safety limit"
            }
            val source = JSONObject(profileJson)
            val dataPlane = source.optString("data_plane", "connect_ip")
            require(dataPlane in setOf("connect_ip", "l4_proxy")) { "Invalid data plane" }
            val id = source.requiredString("id", 64)
            require(profileIdPattern.matches(id)) { "Invalid profile ID" }
            val name = source.requiredString("name", 64)
            require(name.isNotBlank()) { "Invalid profile name" }
            require(VpnReconfigure.tunnelFrontendEnabled(source)) {
                "VpnService only accepts VPN profiles"
            }
            val ipPolicy = source.requiredString("ip_policy", 32)
            require(
                ipPolicy in
                    setOf("automatic", "preferIpv4", "preferIpv6", "ipv4Only", "ipv6Only"),
            ) { "Invalid IP policy" }
            val mtu = source.getInt("mtu")
            require(mtu in 1280..9000) { "MTU is out of range" }
            val dnsMode = source.requiredString("dns_mode", 32)
            require(dnsMode in setOf("tunnel", "localConfigured")) {
                "VPN DNS must be configured on the tunnel"
            }
            val bypassCidrs = source.getJSONArray("bypass_cidrs").strings()
            require(bypassCidrs.size <= VpnRoutePlanner.MAX_GENERATED_ROUTES) {
                "Too many bypass CIDRs"
            }
            val allowLan = source.getBoolean("allow_lan")
            val geoDirectCountries =
                source
                    .optJSONArray("geo_direct_countries")
                    ?.strings()
                    ?.map { country -> country.uppercase() }
                    ?.also { countries ->
                        require(countries.size <= 32 && countries.distinct().size == countries.size) {
                            "Invalid GEO direct countries"
                        }
                        require(countries.all { country -> country.matches(Regex("^[A-Z]{2}$")) }) {
                            "Invalid GEO direct country"
                        }
                    }
                    ?: emptyList()
            val dnsIpv4 =
                parseNumericAddress(source.requiredString("dns_v4", 64), false) as Inet4Address
            val dnsIpv6 =
                parseNumericAddress(source.requiredString("dns_v6", 128), true) as Inet6Address
            val warpDnsMode = source.optJSONObject("warp_dns")?.optString("mode", "plain") ?: "plain"
            require(warpDnsMode in setOf("plain", "doh", "dot")) { "Invalid WARP DNS mode" }
            val ordinaryEncryptedDns = warpDnsMode != "plain" && !ChainProfileFields.enabled(source)
            val endpointIpv4 =
                parseNumericAddress(source.requiredString("endpoint_v4", 64), false) as Inet4Address
            val endpointIpv6 =
                parseNumericAddress(source.requiredString("endpoint_v6", 128), true) as Inet6Address
            val endpointSelection = source.optString("endpoint_selection", "custom")
            require(endpointSelection in setOf("automatic", "custom")) { "Invalid endpoint selection" }
            val configuredDnsServers = listOf(dnsIpv4, dnsIpv6)
            val activeDnsServers = if (ordinaryEncryptedDns) emptyList() else configuredDnsServers
            if (endpointSelection == "custom") {
                require(
                    activeDnsServers.none { server -> server == endpointIpv4 || server == endpointIpv6 },
                ) { "VPN DNS server cannot equal a protected MASQUE endpoint" }
            }
            // Rust hydrates registration-owned endpoints in Custom mode.
            // Automatic selection filters DNS clashes against real candidates
            // in Rust; the dormant custom pair does not describe that pool.
            require(
                configuredDnsServers.none { server ->
                    server.isAnyLocalAddress ||
                        server.isLoopbackAddress ||
                        server.isLinkLocalAddress ||
                        server.isMulticastAddress ||
                        server.hostAddress == "255.255.255.255"
                },
            ) { "VPN DNS server must be a routable unicast address" }
            require(
                activeDnsServers.none { server ->
                    VpnRoutePlanner.isAddressExcluded(server, allowLan, emptyList())
                },
            ) { "VPN DNS server cannot be covered by a LAN or CIDR bypass" }
            // Encrypted DNS uses numeric bootstrap, not physical DNS metadata.
            val directDnsMode =
                source.optJSONObject("direct_dns")?.optString("mode", "physicalSystem") ?: "physicalSystem"
            require(directDnsMode in setOf("physicalSystem", "doh", "dot")) { "Invalid direct DNS mode" }
            val routing = ApplicationRoutingFields.parse(source)
            return AndroidVpnProfile(
                id = id,
                name = name,
                ipPolicy = ipPolicy,
                mtu = mtu,
                dnsMode = dnsMode,
                dataPlane = dataPlane,
                vpnGateEnabled = ChainProfileFields.enabled(source),
                customChain = ChainProfileFields.custom(source),
                proxyChainEnabled = ChainProfileFields.proxy(source),
                dnsIpv4 = dnsIpv4,
                dnsIpv6 = dnsIpv6,
                killSwitch = source.getBoolean("kill_switch"),
                allowLan = allowLan,
                bypassCidrs = bypassCidrs,
                geoDirectCountries = geoDirectCountries,
                bypassDomains = source.optJSONArray("bypass_domains")?.domainStrings() ?: emptyList(),
                directDnsMode = directDnsMode,
                warpDnsMode = warpDnsMode,
                routingDomainRules = routing.needsDns,
                routingDirectDomains = routing.directDns,
            )
        }
    }
}

internal data class WarpAddressAssignment(
    val ipv4: Inet4Address,
    val ipv6: Inet6Address,
) {
    companion object {
        fun parse(json: String): WarpAddressAssignment {
            val source = JSONObject(json)
            return WarpAddressAssignment(
                ipv4 = parseNumericAddress(source.requiredString("ipv4", 64), false) as Inet4Address,
                ipv6 = parseNumericAddress(source.requiredString("ipv6", 128), true) as Inet6Address,
            )
        }
    }
}

/** Negotiated addresses are separate from the outer WARP identity. */
internal data class VpnGateNetwork(
    val ipv4: Inet4Address?,
    val ipv6: Inet6Address?,
    val dns: List<InetAddress>,
    val mtu: Int,
) {
    companion object {
        fun parse(source: JSONObject): VpnGateNetwork {
            val ipv4 =
                if (source.isNull("ipv4")) {
                    null
                } else {
                    parseNumericAddress(source.requiredString("ipv4", 64), false) as Inet4Address
                }
            val ipv6 =
                if (source.isNull("ipv6")) {
                    null
                } else {
                    parseNumericAddress(source.requiredString("ipv6", 128), true) as Inet6Address
                }
            require(ipv4 != null || ipv6 != null) { "VPN Gate assigned no addresses" }
            val mtu = source.getInt("mtu")
            require(mtu in 1280..9000) { "Invalid VPN Gate MTU" }
            val values = source.getJSONArray("dns_servers")
            require(values.length() in 0..8) { "Invalid VPN Gate DNS" }
            val dns =
                List(values.length()) { index ->
                    val text = values.getString(index)
                    parseNumericAddress(text, ':' in text).also { address ->
                        require(!address.isAnyLocalAddress && !address.isLoopbackAddress && !address.isMulticastAddress)
                        require(if (address is Inet4Address) ipv4 != null else ipv6 != null)
                    }
                }
            return VpnGateNetwork(ipv4, ipv6, dns, mtu)
        }
    }
}

private fun JSONObject.requiredString(
    name: String,
    maximumLength: Int,
): String {
    val value = getString(name).trim()
    require(value.isNotEmpty() && value.length <= maximumLength) { "Invalid $name" }
    return value
}

private fun JSONArray.strings(): List<String> =
    List(length()) { index ->
        val value = getString(index).trim()
        require(value.isNotEmpty() && value.length <= 128) { "Invalid bypass CIDR" }
        value
    }

private fun JSONArray.domainStrings(): List<String> =
    List(length()) { index ->
        val value = getString(index).trim()
        val name = value.removeSuffix(".")
        // Match core's pre-IDNA bound; core validates normalized ASCII names and labels.
        require(name.isNotEmpty() && name.codePointCount(0, name.length) <= 253) { "Invalid bypass domain" }
        value
    }

private fun parseNumericAddress(
    value: String,
    ipv6: Boolean,
): InetAddress {
    require(value.isNotBlank() && '%' !in value) { "Invalid IP address" }
    if (ipv6) {
        require(':' in value) { "Expected an IPv6 address" }
        return InetAddress.getByName(value).also {
            require(it is Inet6Address) { "Expected an IPv6 address" }
        }
    }

    val octets = value.split('.')
    require(octets.size == 4) { "Expected an IPv4 address" }
    val bytes =
        ByteArray(4) { index ->
            val text = octets[index]
            require(text.isNotEmpty() && text.length <= 3 && text.all(Char::isDigit)) {
                "Invalid IPv4 address"
            }
            val octet = text.toInt()
            require(octet in 0..255) { "Invalid IPv4 address" }
            octet.toByte()
        }
    return InetAddress.getByAddress(bytes)
}
