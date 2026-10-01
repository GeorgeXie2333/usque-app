package io.github.georgexie2333.usque

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectIpRecoveryPolicyTest {
    private fun failure(
        code: String,
        retryable: Boolean = true,
    ) = ServiceSnapshotState.FailureFields(code = code, stage = "socket_connect", retryable = retryable)

    @Test
    fun structuredRetryableNetworkFailuresCanRecover() {
        for (code in listOf(
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
        )) {
            assertTrue(code, ConnectIpRecoveryPolicy.canRecoverFailure(failure(code), code))
            assertFalse(code, ConnectIpRecoveryPolicy.canRecoverFailure(failure(code, retryable = false), code))
            assertTrue(code, ConnectIpRecoveryPolicy.canRecoverStartup(code))
        }
    }

    @Test
    fun retryableFlagCannotOverrideSecurityConfigurationOrLocalFailures() {
        for (code in listOf(
            "AUTHENTICATION_FAILED",
            "IDENTITY_INVALID",
            "ENDPOINT_PIN_MISMATCH",
            "CONFIGURATION_INVALID",
            "ADDRESS_ASSIGNMENT_INVALID",
            "TUN_ADDRESS_MISSING",
            "SOCKET_PROTECTION_FAILED",
            "DNS_APPLY_FAILED",
            "ROUTE_APPLY_FAILED",
            "KILL_SWITCH_APPLY_FAILED",
            "KILL_SWITCH_STATE_MISMATCH",
            "SYSTEM_PROXY_STATE_MISMATCH",
            "ROUTE_RESTORE_INCOMPLETE",
            "DNS_RESTORE_INCOMPLETE",
            "SYSTEM_PROXY_STALE",
            "PLATFORM_RECOVERY_PENDING",
            "PACKET_SEND_FAILED",
            "PACKET_RECEIVE_FAILED",
            "SEND_QUEUE_FULL",
            "CONNECT_IP_REJECTED",
            "L4_CONNECT_TIMEOUT",
            "INTERNAL",
            "MASQUE_CONNECT_FAILED",
            "ANDROID_RUNTIME_FAILED",
            "UNKNOWN_NETWORK_FAILURE",
            "",
        )) {
            assertFalse(code, ConnectIpRecoveryPolicy.canRecoverFailure(failure(code), code))
            assertFalse(code, ConnectIpRecoveryPolicy.canRecoverStartup(code))
        }
    }

    @Test
    fun missingOrInconsistentFailureEvidenceCannotAuthorizeRecovery() {
        assertFalse(ConnectIpRecoveryPolicy.canRecoverFailure(null, "H3_UDP_UNREACHABLE"))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverFailure(failure("H3_UDP_UNREACHABLE"), null))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverFailure(null, null))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverStartup(null))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverFailure(failure("H3_UDP_UNREACHABLE"), "H2_TCP_CONNECT_FAILED"))
        assertFalse(
            ConnectIpRecoveryPolicy.canRecoverFailure(failure("SOCKET_PROTECTION_FAILED"), "H3_UDP_UNREACHABLE"),
        )
        assertFalse(
            ConnectIpRecoveryPolicy.canRecoverFailure(failure("H3_UDP_UNREACHABLE"), "SOCKET_PROTECTION_FAILED"),
        )
    }

    @Test
    fun physicalNetworkWaitCanContinueAnAdmittedStartupWithoutAuthorizingANewRecovery() {
        val code = "ANDROID_WAITING_FOR_PHYSICAL_NETWORK"
        assertTrue(ConnectIpRecoveryPolicy.canRecoverStartup(code))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverFailure(failure(code), code))
    }
}
