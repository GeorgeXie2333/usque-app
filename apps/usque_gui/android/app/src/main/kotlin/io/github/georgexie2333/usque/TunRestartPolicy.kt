package io.github.georgexie2333.usque

/**
 * Decides whether an Android reconnect may keep the existing VpnService TUN
 * file descriptor. Automatic network recovery or an armed Kill Switch keeps
 * matching addresses/DNS/MTU/routes in place while native restarts.
 */
internal enum class TunRestartDecision {
    RETAIN,
    REPLACE_NEW_FIRST,
    TEARDOWN,
}

internal data class TunIdentity(
    val profileId: String,
    val mtu: Int,
    val dnsMode: String,
    val dnsV4: String,
    val dnsV6: String,
    val allowLan: Boolean,
    val bypassCidrs: List<String>,
    val splitDnsEnabled: Boolean = false,
    val perAppEnabled: Boolean = false,
    val perAppPackages: List<String> = emptyList(),
    val dataPlane: String = "connect_ip",
    val vpnGateEnabled: Boolean = false,
    val warpDnsMode: String = "plain",
) {
    fun sameForReuse(other: TunIdentity): Boolean = this == other

    companion object {
        fun from(
            profile: AndroidVpnProfile,
            perApp: PerAppProxySettings = PerAppProxySettings(),
        ): TunIdentity =
            TunIdentity(
                profileId = profile.id,
                dataPlane = profile.dataPlane,
                vpnGateEnabled = profile.vpnGateEnabled,
                warpDnsMode = profile.warpDnsMode,
                mtu = profile.mtu,
                dnsMode = profile.dnsMode,
                dnsV4 = profile.dnsIpv4.hostAddress ?: profile.dnsIpv4.toString(),
                dnsV6 = profile.dnsIpv6.hostAddress ?: profile.dnsIpv6.toString(),
                allowLan = profile.allowLan,
                bypassCidrs = profile.bypassCidrs,
                splitDnsEnabled = profile.splitDnsEnabled,
                perAppEnabled = perApp.enabled,
                perAppPackages = perApp.packageNames,
            )
    }
}

internal object TunRestartPolicy {
    fun decide(
        killSwitch: Boolean,
        tunnelFrontend: Boolean,
        hasCurrentFd: Boolean,
        sameIdentity: Boolean,
        userRequestedDisconnect: Boolean,
        networkRecovery: Boolean = false,
    ): TunRestartDecision {
        if (userRequestedDisconnect || !tunnelFrontend || !hasCurrentFd || (!killSwitch && !networkRecovery)) {
            return TunRestartDecision.TEARDOWN
        }
        return if (sameIdentity) {
            TunRestartDecision.RETAIN
        } else {
            TunRestartDecision.REPLACE_NEW_FIRST
        }
    }
}
