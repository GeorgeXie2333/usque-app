package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ProtonVpnFieldsTest {
    @Test
    fun ordinaryCommandsRejectCredentialFields() {
        assertEquals("get", JSONObject(ProtonVpnFields.command("""{"action":"get"}""")).getString("action"))
        for (field in listOf("password", "username", "token", "url", "sensitive_input")) {
            assertTrue(
                runCatching {
                    ProtonVpnFields.command(
                        """{"action":"login","$field":"synthetic-private"}""",
                    )
                }.isFailure,
            )
        }
    }

    @Test
    fun localForgetUsesOnlyProfileMetadata() {
        val request = ProtonVpnFields.command("""{"action":"forget","profile_id":"proton-profile"}""")
        assertEquals("forget", JSONObject(request).getString("action"))
        assertEquals("proton-profile", JSONObject(request).getString("profile_id"))
    }

    @Test
    fun metadataIgnoresRawAuthenticationMaterialAtEveryLevel() {
        val source =
            """
            {"job":{"id":"job","state":"waiting_verification","phase":"credentialless",
              "url":"synthetic-private-url","token":"synthetic-private-token",
              "nodes":[{"id":"node","name":"Free","country":"JP","city":"","load":1,
                "private_key":"synthetic-private-key"}]},
             "profiles":[{"id":"profile","name":"Proton","identity":"guest","node_name":"Free",
               "country":"JP","expires_at":1,"needs_login":false,"uid":"synthetic-private-uid"}],
             "password":"synthetic-private-password"}
            """.trimIndent()
        val metadata = ProtonVpnFields.response(source.toByteArray())
        assertFalse(metadata.toString().contains("synthetic-private"))
        assertEquals("waiting_verification", (metadata["job"] as Map<*, *>)["state"])
        assertEquals(1, (metadata["profiles"] as List<*>).size)
    }
}
