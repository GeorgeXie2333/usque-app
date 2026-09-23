package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WarpWireguardFieldsTest {
    @Test
    fun discoveryRepliesAllowOnlyBoundedMetadata() {
        val observation = JSONObject().put("country", "US").put("exit_ip", "104.28.1.1").put("private_key", "hidden")
        val row =
            JSONObject()
                .put(
                    "ipv4",
                    observation,
                ).put("endpoint", JSONObject().put("host", "162.159.192.1").put("port", 500))
        val source = JSONObject().put("results", JSONArray().put(row)).put("token", "hidden")
        val result = WarpWireguardFields.response(source.toString())
        assertFalse(result.toString().contains("hidden"))
        assertTrue(result.toString().contains("104.28.1.1"))
        source.put("results", JSONArray(List(257) { row }))
        assertTrue(runCatching { WarpWireguardFields.response(source.toString()) }.isFailure)
    }

    @Test
    fun endpointChangesParticipateInSelectionIdentity() {
        val endpoint = JSONObject().put("host", "162.159.192.1").put("port", 2408)
        val chain =
            JSONObject()
                .put("enabled", true)
                .put("source", "warp_wireguard")
                .put("profile_id", "id")
                .put("revision", "revision")
                .put("endpoint_override", endpoint)
        val source = JSONObject().put("chain_exit", chain)
        val first = ChainProfileFields.selection(source)
        endpoint.put("port", 500)
        val second = ChainProfileFields.selection(source)
        assertFalse(first == second)
        chain.put("endpoint_override", JSONObject().put("port", 500).put("host", "162.159.192.1"))
        assertEquals(second, ChainProfileFields.selection(source))
    }
}
