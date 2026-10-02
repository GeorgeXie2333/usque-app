package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectIpRecoveryPolicyTest {
    private fun failure(
        code: String,
        retryable: Boolean = true,
    ) = ServiceSnapshotState.FailureFields(code = code, stage = "socket_connect", retryable = retryable)

    private fun gate(
        reason: Any? = null,
        stage: String = "error",
    ): JSONObject = JSONObject().put("stage", stage).put("failure", reason ?: JSONObject.NULL)

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
            assertTrue(code, ConnectIpRecoveryPolicy.canRecoverStartup(code, failure(code)))
            assertFalse(code, ConnectIpRecoveryPolicy.canRecoverStartup(code))
            assertFalse(code, ConnectIpRecoveryPolicy.canRecoverStartup(code, failure(code, retryable = false)))
            assertTrue(code, ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, null))
            assertTrue(code, ConnectIpRecoveryPolicy.canRecoverChainStartup(code, failure(code), null))
            assertFalse(code, ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code, false), code, null))
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
            assertFalse(code, ConnectIpRecoveryPolicy.canRecoverStartup(code, failure(code)))
            if (code != "PACKET_RECEIVE_FAILED") {
                assertFalse(
                    code,
                    ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, gate("transport")),
                )
                assertFalse(
                    code,
                    ConnectIpRecoveryPolicy.canRecoverChainStartup(code, failure(code), gate("transport")),
                )
            }
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
        assertFalse(ConnectIpRecoveryPolicy.canRecoverStartup(code, failure(code)))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverFailure(failure(code), code))
    }

    @Test
    fun ordinaryStartupCannotUseMissingTerminalOrInconsistentNativeEvidence() {
        val code = "H3_UDP_UNREACHABLE"
        assertFalse(ConnectIpRecoveryPolicy.canRecoverStartup(code, null))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverStartup(code, failure(code, retryable = false)))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverStartup(code, failure("H3_CONNECTION_CLOSED")))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverStartup("SOCKET_PROTECTION_FAILED", failure(code)))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverStartup(code, failure("SOCKET_PROTECTION_FAILED")))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverStartup(null, failure(code)))
        assertFalse(
            ConnectIpRecoveryPolicy.canRecoverStartup("ANDROID_WAITING_FOR_PHYSICAL_NETWORK", failure(code)),
        )
    }

    @Test
    fun chainTransportPacketFailureRequiresConsistentTypedGateEvidence() {
        val code = "PACKET_RECEIVE_FAILED"
        val transportGate = gate("transport")
        assertTrue(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, transportGate))
        assertTrue(ConnectIpRecoveryPolicy.canRecoverChainStartup(code, failure(code), transportGate))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverFailure(failure(code), code))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, null))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, gate()))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, gate("transport", "connected")))
        assertFalse(
            ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, gate("transport", "negotiating")),
        )
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(null, code, transportGate))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code, false), code, transportGate))
        assertFalse(
            ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), "H3_CONNECTION_CLOSED", transportGate),
        )
        assertFalse(
            ConnectIpRecoveryPolicy.canRecoverChainFailure(failure("H3_CONNECTION_CLOSED"), code, transportGate),
        )
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainStartup(code, null, transportGate))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainStartup("MASQUE_CONNECT_FAILED", null, transportGate))
    }

    @Test
    fun terminalUnknownAndMalformedGateReasonsOverrideOtherwiseRetryableNetworkEvidence() {
        val reasons =
            listOf(
                "authentication",
                "certificate",
                "configuration",
                "protocol",
                "address_changed",
                "cleanup",
                "unknown",
                "",
                "TRANSPORT",
                true,
                1,
                JSONObject().put("reason", "transport"),
            )
        for (reason in reasons) {
            for (code in listOf("H3_CONNECTION_CLOSED", "PACKET_RECEIVE_FAILED")) {
                assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, gate(reason)))
                assertFalse(ConnectIpRecoveryPolicy.canRecoverChainStartup(code, failure(code), gate(reason)))
            }
            assertFalse(
                ConnectIpRecoveryPolicy.canRecoverChainStartup(
                    "ANDROID_WAITING_FOR_PHYSICAL_NETWORK",
                    null,
                    gate(reason),
                ),
            )
        }
    }

    @Test
    fun chainUnderlayFailureMayPrecedeAnyFinalGateFailure() {
        val code = "PHYSICAL_NETWORK_CHANGED"
        for (status in listOf(null, JSONObject(), gate(stage = "connecting_warp"), gate("transport"))) {
            assertTrue(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, status))
            assertTrue(ConnectIpRecoveryPolicy.canRecoverChainStartup(code, failure(code), status))
        }
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), null, gate("transport")))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainStartup(code, null, gate("transport")))
        assertFalse(
            ConnectIpRecoveryPolicy.canRecoverChainStartup(
                "SOCKET_PROTECTION_FAILED",
                failure("SOCKET_PROTECTION_FAILED"),
                gate("transport"),
            ),
        )
    }

    @Test
    fun chainL4SessionLossRequiresTypedRetryableEvidenceAndCannotHideTerminalFailures() {
        val code = "L4_SESSION_UNAVAILABLE"
        for (status in listOf(null, gate(stage = "connecting_warp"), gate("transport"))) {
            assertTrue(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, status))
            assertTrue(ConnectIpRecoveryPolicy.canRecoverChainStartup(code, failure(code), status))
            assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code, false), code, status))
        }
        assertFalse(ConnectIpRecoveryPolicy.canRecoverFailure(failure(code), code))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverStartup(code))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(null, code, null))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainStartup(code, null, null))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), "H3_CONNECTION_CLOSED", null))
        for (reason in listOf("authentication", "certificate", "configuration", "protocol", "cleanup")) {
            assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, gate(reason)))
        }
        for (terminal in listOf(
            "AUTHENTICATION_FAILED",
            "ENDPOINT_PIN_MISMATCH",
            "SOCKET_PROTECTION_FAILED",
            "L4_PROTOCOL_ERROR",
            "L4_CONNECT_REJECTED",
            "L4_RESOURCE_EXHAUSTED",
        )) {
            assertFalse(ConnectIpRecoveryPolicy.canRecoverChainStartup(terminal, failure(terminal), gate("transport")))
        }
    }

    @Test
    fun chainPhysicalNetworkWaitUsesOnlyTheExactServiceCodeWithoutNativeFailureEvidence() {
        val code = "ANDROID_WAITING_FOR_PHYSICAL_NETWORK"
        assertTrue(ConnectIpRecoveryPolicy.canRecoverChainStartup(code, null, null))
        assertTrue(ConnectIpRecoveryPolicy.canRecoverChainStartup(code, null, gate(stage = "connecting_warp")))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainStartup(code, failure("H3_CONNECTION_CLOSED"), null))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainStartup(null, null, null))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverChainFailure(failure(code), code, gate("transport")))
    }

    @Test
    fun everyChainSourceIsEligibleWithConnectIpOrL4Underlay() {
        val sources =
            listOf("openvpn_custom", "wireguard_custom", "warp_wireguard", "vpn_gate", "http_proxy", "socks5_proxy")
        for (source in sources) {
            for (dataPlane in listOf("connect_ip", "l4_proxy")) {
                val profile =
                    JSONObject()
                        .put("data_plane", dataPlane)
                        .put("chain_exit", JSONObject().put("source", source).put("enabled", true))
                assertTrue("$source/$dataPlane", ConnectIpRecoveryPolicy.canRecoverProfile(profile))
            }
        }
        assertTrue(
            ConnectIpRecoveryPolicy.canRecoverProfile(
                JSONObject().put("data_plane", "l4_proxy").put("vpn_gate", JSONObject().put("enabled", true)),
            ),
        )
    }

    @Test
    fun plainL4DisabledChainAndUnsupportedDataPlaneRemainIneligible() {
        assertFalse(ConnectIpRecoveryPolicy.canRecoverProfile(null))
        assertTrue(ConnectIpRecoveryPolicy.canRecoverProfile(JSONObject()))
        assertTrue(ConnectIpRecoveryPolicy.canRecoverProfile(JSONObject().put("data_plane", "connect_ip")))
        assertFalse(ConnectIpRecoveryPolicy.canRecoverProfile(JSONObject().put("data_plane", "l4_proxy")))
        assertFalse(
            ConnectIpRecoveryPolicy.canRecoverProfile(
                JSONObject()
                    .put("data_plane", "l4_proxy")
                    .put("chain_exit", JSONObject().put("source", "http_proxy").put("enabled", false))
                    .put("vpn_gate", JSONObject().put("enabled", true)),
            ),
        )
        assertFalse(
            ConnectIpRecoveryPolicy.canRecoverProfile(
                JSONObject()
                    .put("data_plane", "unknown")
                    .put("chain_exit", JSONObject().put("source", "socks5_proxy").put("enabled", true)),
            ),
        )
    }
}
