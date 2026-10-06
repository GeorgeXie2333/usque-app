package io.github.georgexie2333.usque

import org.json.JSONObject

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

    /** A failed protected socket is discarded; an established session may retry on a newer network. */
    fun canRecoverOnNetworkChange(
        failure: ServiceSnapshotState.FailureFields?,
        errorCode: String?,
        establishedSession: Boolean,
    ): Boolean =
        establishedSession && errorCode == "SOCKET_PROTECTION_FAILED" &&
            failure?.code == errorCode && failure.stage == "socket_protection"

    /** Only continue a previously admitted recovery; this cannot authorize an initial restart. */
    fun canRecoverStartup(
        code: String?,
        failure: ServiceSnapshotState.FailureFields? = null,
    ): Boolean =
        if (failure != null) {
            canRecoverFailure(failure, code)
        } else {
            code == "ANDROID_WAITING_FOR_PHYSICAL_NETWORK"
        }

    /** Source-independent eligibility; the service separately validates the full profile and TUN. */
    fun canRecoverProfile(profile: JSONObject?): Boolean =
        profile != null &&
            when (profile.optString("data_plane", "connect_ip")) {
                "connect_ip" -> true
                "l4_proxy" -> ChainProfileFields.enabled(profile)
                else -> false
            }

    /** A chain may have an underlay failure, or the shared final gate's precise transport failure. */
    fun canRecoverChainFailure(
        failure: ServiceSnapshotState.FailureFields?,
        errorCode: String?,
        gateStatus: JSONObject?,
    ): Boolean {
        if (failure == null || !failure.retryable || failure.code != errorCode || !recoverableGate(gateStatus)) {
            return false
        }
        // L4 maps only H3 reachability, handshake, closure and PMTU errors to this code.
        if (failure.code in networkFailures || failure.code == "L4_SESSION_UNAVAILABLE") return true
        // GateFailure::Transport maps to this code for every chain source. A generic
        // packet error without the gate's typed transport reason is not sufficient.
        return failure.code == "PACKET_RECEIVE_FAILED" &&
            gateStatus != null && gateStatus.opt("failure") == "transport" && gateStatus.opt("stage") == "error"
    }

    /** Continue only an already admitted chain recovery, using the current attempt's native evidence. */
    fun canRecoverChainStartup(
        code: String?,
        failure: ServiceSnapshotState.FailureFields?,
        gateStatus: JSONObject?,
    ): Boolean =
        if (code == "ANDROID_WAITING_FOR_PHYSICAL_NETWORK") {
            // This exact service-side wait precedes native startup, so it has no native failure body.
            failure == null && recoverableGate(gateStatus)
        } else {
            canRecoverChainFailure(failure, code, gateStatus)
        }

    private fun recoverableGate(gateStatus: JSONObject?): Boolean {
        val reason = gateStatus?.opt("failure")
        return reason == null || reason == JSONObject.NULL || reason == "transport"
    }
}
