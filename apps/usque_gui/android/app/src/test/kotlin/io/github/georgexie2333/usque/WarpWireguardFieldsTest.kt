package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WarpWireguardFieldsTest {
    @Test
    fun nativeFailuresKeepSafeCodesWithoutExceptionDetails() {
        assertEquals("identity_required", WarpWireguardFields.failureCode("WARP_IDENTITY_REQUIRED"))
        assertEquals("identity_invalid", WarpWireguardFields.failureCode("VPN_GATE_IDENTITY_INVALID"))
        assertEquals("secure_storage_failed", WarpWireguardFields.failureCode("CHAIN_CRYPTO_UNAVAILABLE"))
        assertEquals("unavailable", WarpWireguardFields.failureCode("Bearer private-token"))
        assertEquals("unavailable", WarpWireguardFields.failureCode(null))
    }

    @Test
    fun generationRepliesAllowOnlyBoundedStatusFields() {
        val job =
            JSONObject()
                .put("id", "job")
                .put("state", "completed")
                .put("profile_id", "saved")
                .put("private_key", "hidden")
        val source =
            JSONObject()
                .put("job", job)
                .put("token", "hidden")
                .put("results", "discarded")
                .put("history", "discarded")
        val result = WarpWireguardFields.response(source.toString())
        assertEquals(setOf("job", "error"), result.keys)
        assertFalse(result.toString().contains("hidden"))
        assertFalse(result.toString().contains("discarded"))
        assertEquals("saved", (result["job"] as Map<*, *>)["profile_id"])
        job.put("profile_id", "a".repeat(513))
        assertTrue(runCatching { WarpWireguardFields.response(source.toString()) }.isFailure)
        job.put("profile_id", JSONObject())
        assertTrue(runCatching { WarpWireguardFields.response(source.toString()) }.isFailure)
        assertTrue(runCatching { WarpWireguardFields.response(" ".repeat(4097)) }.isFailure)
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
