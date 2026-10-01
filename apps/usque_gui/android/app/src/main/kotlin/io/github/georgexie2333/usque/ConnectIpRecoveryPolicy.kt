package io.github.georgexie2333.usque

/** Failure eligibility only; the service owns profile, TUN, generation and cleanup admission. */
internal object ConnectIpRecoveryPolicy {
    private val networkFailures =
        setOf(
            "PHYSICAL_IPV4_UNAVAILABLE",
            "PHYSICAL_IPV6_UNAVAILABLE",
            "PHYSICAL_DNS_UNAVAILABLE",
            "PHYSICAL_NETWORK_CHANGED",
            "SOCKET_AFFINITY_INVALID",
            "H3_UDP_UNREACHABLE",
            "H3_HANDSHAKE_TIMEOUT",
            "H3_PROTOCOL_ERROR",
            "H3_DATAGRAM_UNAVAILABLE",
            "H3_CONNECTION_CLOSED",
            "PMTU_REVALIDATION_EXHAUSTED",
            "H2_TCP_CONNECT_FAILED",
            "H2_TLS_FAILED",
            "H2_STREAM_CLOSED",
            "H2_CONNECT_REJECTED",
            "H2_GOAWAY",
            "ALL_TRANSPORTS_FAILED",
            "PACKET_SEND_TIMEOUT",
            "PACKET_RECEIVE_STALLED",
        )

    /** Unknown, missing, inconsistent or terminal failures never authorize a restart. */
    fun canRecoverFailure(
        failure: ServiceSnapshotState.FailureFields?,
        errorCode: String?,
    ): Boolean = failure != null && failure.retryable && failure.code == errorCode && failure.code in networkFailures

    /** Only continue a previously admitted recovery; this cannot authorize an initial restart. */
    fun canRecoverStartup(code: String?): Boolean =
        code != null && (code == "ANDROID_WAITING_FOR_PHYSICAL_NETWORK" || code in networkFailures)
}
