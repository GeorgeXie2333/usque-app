package io.github.georgexie2333.usque

import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress

/**
 * Android 13 can express exclusions directly. Older releases require the
 * complement of every bypass CIDR to be installed as positive VPN routes.
 */
internal object VpnRoutePlanner {
    const val MAX_GENERATED_ROUTES = 256

    private val lanExclusions =
        listOf(
            CidrBlock.parse("10.0.0.0/8"),
            CidrBlock.parse("172.16.0.0/12"),
            CidrBlock.parse("192.168.0.0/16"),
            CidrBlock.parse("169.254.0.0/16"),
            CidrBlock.parse("fc00::/7"),
            CidrBlock.parse("fe80::/10"),
        )

    fun isAddressExcluded(
        address: InetAddress,
        allowLan: Boolean,
        bypassCidrs: List<String>,
    ): Boolean =
        ((if (allowLan) lanExclusions else emptyList()) + bypassCidrs.map(CidrBlock::parse))
            .any { block -> block.contains(address) }

    fun plan(
        includeIpv4: Boolean,
        includeIpv6: Boolean,
        allowLan: Boolean,
        bypassCidrs: List<String>,
        supportsRouteExclusion: Boolean,
    ): RoutePlan {
        require(includeIpv4 || includeIpv6) { "At least one address family must be enabled" }
        require(bypassCidrs.size <= MAX_GENERATED_ROUTES) { "Too many bypass CIDRs" }

        val defaults =
            buildList {
                if (includeIpv4) add(CidrBlock.parse("0.0.0.0/0"))
                if (includeIpv6) add(CidrBlock.parse("::/0"))
            }
        val exclusions =
            ((if (allowLan) lanExclusions else emptyList()) + bypassCidrs.map(CidrBlock::parse))
                .filter { block ->
                    (block.isIpv4 && includeIpv4) || (block.isIpv6 && includeIpv6)
                }
                .distinctBy(CidrBlock::key)

        require(exclusions.none { it.prefixLength == 0 }) {
            "A /0 bypass would disable VPN protection for an address family"
        }

        if (supportsRouteExclusion) {
            return RoutePlan(included = defaults, excluded = exclusions)
        }

        var included = defaults
        for (exclusion in exclusions) {
            included = included.flatMap { route -> route.subtract(exclusion) }
            require(included.size <= MAX_GENERATED_ROUTES) {
                "Bypass CIDRs require more than $MAX_GENERATED_ROUTES Android VPN routes"
            }
        }
        return RoutePlan(included = included, excluded = emptyList())
    }
}

internal data class RoutePlan(
    val included: List<CidrBlock>,
    val excluded: List<CidrBlock>,
)

internal data class CidrBlock private constructor(
    val address: InetAddress,
    val prefixLength: Int,
) {
    val isIpv4: Boolean
        get() = address is Inet4Address

    val isIpv6: Boolean
        get() = address is Inet6Address

    val key: String
        get() = "${address.hostAddress}/$prefixLength"

    fun subtract(exclusion: CidrBlock): List<CidrBlock> {
        if (!overlaps(exclusion)) return listOf(this)
        if (exclusion.contains(this)) return emptyList()
        if (!contains(exclusion)) return listOf(this)

        val retained = mutableListOf<CidrBlock>()
        var current = this
        while (current.prefixLength < exclusion.prefixLength) {
            val (left, right) = current.split()
            if (left.contains(exclusion)) {
                retained += right
                current = left
            } else {
                retained += left
                current = right
            }
        }
        return retained
    }

    fun contains(other: CidrBlock): Boolean {
        if (address.address.size != other.address.address.size || prefixLength > other.prefixLength) {
            return false
        }
        return prefixMatches(address.address, other.address.address, prefixLength)
    }

    fun contains(other: InetAddress): Boolean {
        if (address.address.size != other.address.size) return false
        return prefixMatches(address.address, other.address, prefixLength)
    }

    private fun overlaps(other: CidrBlock): Boolean =
        contains(other) || other.contains(this)

    private fun split(): Pair<CidrBlock, CidrBlock> {
        val bitCount = address.address.size * 8
        require(prefixLength < bitCount) { "A host route cannot be split" }
        val leftBytes = address.address.copyOf()
        val rightBytes = leftBytes.copyOf()
        val byteIndex = prefixLength / 8
        val bitMask = 1 shl (7 - (prefixLength % 8))
        rightBytes[byteIndex] = (rightBytes[byteIndex].toInt() or bitMask).toByte()
        return create(leftBytes, prefixLength + 1) to create(rightBytes, prefixLength + 1)
    }

    companion object {
        fun parse(value: String): CidrBlock {
            val normalized = value.trim()
            require(normalized.length in 3..128 && normalized.count { it == '/' } == 1) {
                "Invalid bypass CIDR"
            }
            val (host, prefixText) = normalized.split('/', limit = 2)
            require('%' !in host && host.isNotBlank()) { "Scoped addresses are not supported" }
            val bytes =
                if (':' in host) {
                    InetAddress.getByName(host).also {
                        require(it is Inet6Address) { "Invalid IPv6 CIDR" }
                    }.address
                } else {
                    parseIpv4(host)
                }
            val prefix = prefixText.toIntOrNull() ?: error("Invalid CIDR prefix")
            require(prefix in 0..(bytes.size * 8)) { "CIDR prefix is out of range" }
            return create(bytes, prefix)
        }

        private fun create(source: ByteArray, prefixLength: Int): CidrBlock {
            val network = source.copyOf()
            val fullBytes = prefixLength / 8
            val remainingBits = prefixLength % 8
            if (remainingBits != 0 && fullBytes < network.size) {
                val mask = 0xff shl (8 - remainingBits)
                network[fullBytes] = (network[fullBytes].toInt() and mask).toByte()
            }
            val clearFrom = fullBytes + if (remainingBits == 0) 0 else 1
            for (index in clearFrom until network.size) {
                network[index] = 0
            }
            return CidrBlock(InetAddress.getByAddress(network), prefixLength)
        }

        private fun parseIpv4(host: String): ByteArray {
            val octets = host.split('.')
            require(octets.size == 4) { "Invalid IPv4 CIDR" }
            return ByteArray(4) { index ->
                val text = octets[index]
                require(text.isNotEmpty() && text.length <= 3 && text.all(Char::isDigit)) {
                    "Invalid IPv4 CIDR"
                }
                val value = text.toInt()
                require(value in 0..255) { "Invalid IPv4 CIDR" }
                value.toByte()
            }
        }

        private fun prefixMatches(left: ByteArray, right: ByteArray, prefixLength: Int): Boolean {
            val wholeBytes = prefixLength / 8
            for (index in 0 until wholeBytes) {
                if (left[index] != right[index]) return false
            }
            val remainingBits = prefixLength % 8
            if (remainingBits == 0) return true
            val mask = 0xff shl (8 - remainingBits)
            return (left[wholeBytes].toInt() and mask) == (right[wholeBytes].toInt() and mask)
        }
    }
}
