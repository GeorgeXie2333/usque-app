package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ChainProfileFieldsTest {
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
