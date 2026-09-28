package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ChainProfileFieldsTest {
    @Test
    fun candidateAndAttemptMetadataHaveNestedSecretAllowlists() {
        val endpoint = JSONObject().put("host", "2001:db8::1").put("port", 1194).put("password", "hidden")
        val candidate = JSONObject().put("endpoint", endpoint).put("ipv6", true).put("private_key", "hidden")
        val profile = JSONObject().put("candidates", JSONArray().put(candidate)).put("remote_random", true)
        val summary = ChainProfileFields.summary(profile)!!
        assertFalse(summary.toString().contains("hidden"))
        assertEquals(true, summary["remote_random"])
        val status =
            VpnGateFields.status(
                JSONObject()
                    .put(
                        "attempting_endpoint",
                        endpoint,
                    ).put("attempt_count", 2)
                    .put("candidate_count", 5)
                    .put("active_endpoint", "[2001:db8::1]:1194"),
            )!!
        assertFalse(status.toString().contains("hidden"))
        assertEquals(2, status["attempt_count"])
        assertEquals("[2001:db8::1]:1194", status["active_endpoint"])
        profile.put("candidates", JSONArray(List(17) { candidate }))
        assertTrue(runCatching { ChainProfileFields.summary(profile) }.isFailure)
    }

    @Test
    fun gateSelectionIdentityUsesTheNodeAndConfigurationHash() {
        val chain = JSONObject().put("enabled", true).put("source", "vpn_gate")
        val selected = JSONObject().put("server_id", "first").put("config_sha256", "a")
        val profile = JSONObject().put("chain_exit", chain).put("vpn_gate", JSONObject().put("selection", selected))
        val first = ChainProfileFields.selection(profile)
        selected.put("server_id", "second")
        val second = ChainProfileFields.selection(profile)
        assertFalse(first == second)
        selected.put("config_sha256", "b")
        assertFalse(second == ChainProfileFields.selection(profile))
        val reordered = JSONObject().put("vpn_gate", profile.getJSONObject("vpn_gate")).put("chain_exit", chain)
        assertEquals(ChainProfileFields.selection(profile), ChainProfileFields.selection(reordered))
    }

    @Test
    fun importedExitWinsOverDormantLegacySwitch() {
        val profile = JSONObject().put("vpn_gate", JSONObject().put("enabled", true))
        assertTrue(ChainProfileFields.enabled(profile))
        profile.put("chain_exit", JSONObject().put("source", "wireguard_custom").put("enabled", false))
        assertFalse(ChainProfileFields.enabled(profile))
        assertTrue(ChainProfileFields.selection(profile)!!.contains("wireguard_custom"))
    }

    @Test
    fun profileAndStatusKeepMetadataButNeverConfigurationOrCredentials() {
        val summary =
            JSONObject()
                .put("id", "saved-id")
                .put("revision", "r")
                .put("edit_revision", "e")
                .put("name", "Office")
                .put("protocol", "wireguard")
                .put("endpoint", JSONObject().put("host", "vpn.example").put("port", 51820).put("password", "hidden"))
                .put("configuration", "hidden")
                .put("private_key", "hidden")
                .put("password", "hidden")
        val response =
            ChainProfileFields.response(
                JSONObject().put("profiles", JSONArray().put(summary)).put("preview", summary).toString(),
            )
        assertFalse(response.toString().contains("hidden"))
        assertEquals("e", (response["preview"] as Map<*, *>)["edit_revision"])
        val status =
            VpnGateFields.status(
                JSONObject().put("stage", "connected").put("current_profile", summary).put("dns_unavailable", true),
            )!!
        assertFalse(status.toString().contains("hidden"))
        assertEquals(true, status["dns_unavailable"])
    }
}
