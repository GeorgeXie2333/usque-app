package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class VpnGateFieldsTest {
    @Test
    fun directoryOnlyExportsMetadataAndRetainsMissingMetrics() {
        val server =
            JSONObject()
                .put("id", "v1:one")
                .put("ip", "203.0.113.1")
                .put("config_sha256", "hash")
                .put("ping_ms", JSONObject.NULL)
                .put("openvpn_config_base64", "must-not-cross-bridge")
        val wire =
            VpnGateFields.directory(
                JSONObject()
                    .put("servers", JSONArray().put(server))
                    .put("countries", JSONArray())
                    .put("saved_server", server)
                    .put("cached", true)
                    .toString(),
            )
        val node = (wire["servers"] as List<*>).single() as Map<*, *>
        assertFalse(node.containsKey("openvpn_config_base64"))
        assertEquals("hash", node["config_sha256"])
        assertNull(node["ping_ms"])
        assertEquals(node, wire["saved_server"])
        assertEquals(true, wire["cached"])
    }

    @Test
    fun statusRoundTripKeepsIndependentStagesAndIpv4OnlyNetwork() {
        val raw =
            JSONObject()
                .put("stage", "configuring_network")
                .put("warp_stage", "connected")
                .put("generation", 19)
                .put("secret", "hidden")
                .put(
                    "network",
                    JSONObject()
                        .put("ipv4", "10.8.0.2")
                        .put("ipv6", JSONObject.NULL)
                        .put("mtu", 1400)
                        .put("dns_servers", JSONArray().put("1.1.1.1")),
                )
        val snapshot = ServiceSnapshotState()
        snapshot.applyNativeSnapshot(JSONObject().put("phase", "reconnecting").put("vpn_gate", raw))
        val decoded = VpnGateFields.decodeStatus(snapshot.vpnGateJson)!!
        assertEquals("connected", decoded["warp_stage"])
        assertEquals("configuring_network", decoded["stage"])
        assertEquals(19, decoded["generation"])
        assertFalse(decoded.containsKey("secret"))
        assertNull((decoded["network"] as Map<*, *>)["ipv6"])
        snapshot.reset("disconnected")
        assertNull(snapshot.vpnGateJson)
    }

    @Test
    fun malformedOrOversizedStatusIsRejected() {
        assertNull(VpnGateFields.decodeStatus("{"))
        assertNull(VpnGateFields.decodeStatus(" ".repeat(16 * 1024 + 1)))
        assertTrue(
            runCatching {
                val nodes = JSONArray()
                repeat(101) { nodes.put(JSONObject()) }
                VpnGateFields.directory(JSONObject().put("servers", nodes).put("countries", JSONArray()).toString())
            }.isFailure,
        )
    }
}
