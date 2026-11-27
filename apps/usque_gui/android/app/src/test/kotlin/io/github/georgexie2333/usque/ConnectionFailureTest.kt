package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectionFailureTest {
    @Test
    fun nonErrorReconnectingSnapshotDoesNotMaskAlreadyRunningCommandRejection() {
        val reconnectingSnapshot =
            JSONObject()
                .put("phase", "reconnecting")
                .put("warning", "VPN Gate platform configuration pending")
                .put("error_code", "PACKET_RECEIVE_FAILED")
                .put(
                    "failure",
                    JSONObject()
                        .put("code", "PACKET_RECEIVE_FAILED")
                        .put("stage", "packet_receive")
                        .put("retryable", true),
                ).put(
                    "vpn_gate",
                    JSONObject()
                        .put("stage", "configuring_network")
                        .put("failure", JSONObject.NULL)
                        .put("current_profile", JSONObject().put("source", "proton_vpn")),
                )

        val resolved =
            ConnectionFailure.fromNativeStartResult(
                NativeEngine.ERROR_ALREADY_RUNNING,
                reconnectingSnapshot,
            )

        assertEquals("ANDROID_RUNTIME_FAILED", resolved.code)
        assertEquals("Another native data channel is already running.", resolved.message)
        assertNull(resolved.gateStatus)
        assertNull(resolved.details)
    }

    @Test
    fun nonErrorConnectedWarningDoesNotOverrideCommandFallback() {
        val connectedSnapshot =
            JSONObject()
                .put("phase", "connected")
                .put("warning", "ADS_UNAVAILABLE")

        val resolved =
            ConnectionFailure.fromNativeStartResult(
                NativeEngine.ERROR_INVALID_PROFILE,
                connectedSnapshot,
            )

        assertEquals("ANDROID_RUNTIME_FAILED", resolved.code)
        assertEquals("The Rust engine rejected this profile.", resolved.message)
        assertNull(resolved.gateStatus)
        assertNull(resolved.details)
    }

    @Test
    fun errorSnapshotPreservesRecoverableProtonFailureAndGateStatus() {
        val errorSnapshot =
            JSONObject()
                .put("phase", "error")
                .put("warning", "VPN Gate connection failed (Proton)")
                .put("error_code", "PROTON_TRANSPORT_FAILED")
                .put(
                    "failure",
                    JSONObject()
                        .put("code", "PROTON_TRANSPORT_FAILED")
                        .put("stage", "tls_handshake")
                        .put("retryable", true),
                ).put(
                    "vpn_gate",
                    JSONObject()
                        .put("stage", "error")
                        .put("failure", JSONObject().put("proton", JSONObject().put("kind", "transport")))
                        .put("current_profile", JSONObject().put("source", "proton_vpn")),
                )

        val resolved =
            ConnectionFailure.fromNativeStartResult(
                NativeEngine.ERROR_TRANSPORT_FAILURE,
                errorSnapshot,
            )

        assertEquals("PROTON_TRANSPORT_FAILED", resolved.code)
        assertEquals("VPN Gate connection failed (Proton)", resolved.message)
        assertTrue(resolved.details?.retryable == true)
        val gate = JSONObject(requireNotNull(resolved.gateStatus))
        assertTrue(ConnectIpRecoveryPolicy.canRecoverChainFailure(resolved.details, resolved.code, gate))
        assertFalse(
            ConnectIpRecoveryPolicy.canRecoverChainFailure(
                resolved.details?.copy(code = "PACKET_RECEIVE_FAILED"),
                "PACKET_RECEIVE_FAILED",
                gate,
            ),
        )
    }
}
