package io.github.georgexie2333.usque

/**
 * Decides whether an Android reconnect may keep the existing VpnService TUN
 * file descriptor. Kill Switch armed + same addresses/DNS/MTU/routes retains
 * the fd so physical traffic stays black-holed while native restarts.
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
) {
    fun sameForReuse(other: TunIdentity): Boolean = this == other

    companion object {
        fun from(profile: AndroidVpnProfile): TunIdentity =
            TunIdentity(
                profileId = profile.id,
                mtu = profile.mtu,
                dnsMode = profile.dnsMode,
                dnsV4 = profile.dnsIpv4.hostAddress ?: profile.dnsIpv4.toString(),
                dnsV6 = profile.dnsIpv6.hostAddress ?: profile.dnsIpv6.toString(),
                allowLan = profile.allowLan,
                bypassCidrs = profile.bypassCidrs,
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
    ): TunRestartDecision {
        if (userRequestedDisconnect || !tunnelFrontend || !hasCurrentFd || !killSwitch) {
            return TunRestartDecision.TEARDOWN
        }
        return if (sameIdentity) {
            TunRestartDecision.RETAIN
        } else {
            TunRestartDecision.REPLACE_NEW_FIRST
        }
    }
}
