package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ScenarioFieldsTest {
    @Test
    fun scenePreflightRejectsUnavailableStartupAuthorityBeforeInspectingSecrets() {
        val target = JSONObject().put("id", "zero-trust").put("endpoint_v4", "192.0.2.1")
        val source = JSONObject().put("preview_target", target)
        var inspected = false
        val result =
            runCatching {
                ScenarioFields.validateIdentity(
                    source,
                    loadSecret = { id, profile ->
                        assertEquals("zero-trust", id)
                        assertEquals("192.0.2.1", JSONObject(profile).getString("endpoint_v4"))
                        // The startup resolver rejects missing managed endpoints.
                        null
                    },
                    inspect = { inspected = true },
                )
            }
        assertEquals("SCENARIO_ACCOUNT_UNAVAILABLE", result.exceptionOrNull()?.message)
        assertFalse(inspected)
    }

    @Test
    fun scenePreflightInspectsAuthoritativeRollbackIdentityAndClearsEveryBuffer() {
        for (invalid in listOf(false, true)) {
            val rollback = byteArrayOf(4, 2)
            val result =
                runCatching {
                    ScenarioFields.validateIdentity(
                        JSONObject().put("preview_target", JSONObject().put("id", "armed-account")),
                        loadSecret = { _, _ -> rollback },
                        inspect = {
                            assertTrue(it.contentEquals(byteArrayOf(4, 2)))
                            if (invalid) error("private identity detail")
                        },
                    )
                }
            assertTrue(rollback.all { it == 0.toByte() })
            if (invalid) {
                assertEquals("SCENARIO_ACCOUNT_UNAVAILABLE", result.exceptionOrNull()?.message)
            } else {
                assertEquals("armed-account", result.getOrThrow())
            }
        }
    }

    @Test
    fun privateRuntimeTargetsNeverLeaveTheVpnHost() {
        val value =
            ScenarioFields.decode(
                """{"scenarios":[],"persisted":true,"target":{"id":"private"},"preview_target":{"id":"private"},"network_settings":{"target":{}}}""",
            )
        assertEquals(true, value["persisted"])
        assertFalse(value.containsKey("target"))
        assertFalse(value.containsKey("preview_target"))
        assertFalse(value.containsKey("network_settings"))
    }

    @Test
    fun credentialPreflightCannotAccidentallyCommitTheOriginalRequest() {
        val activation =
            JSONObject()
                .put("action", "activate")
                .put("id", "scenario")
                .put("expected_revision", "revision")
                .put("operation_id", "operation")
                .put("preview_token", "token")
        val preview = ScenarioFields.previewRequest(activation)
        assertEquals("preview", preview.getString("action"))
        assertEquals("scenario", preview.getString("id"))
        assertFalse(preview.has("operation_id"))
        assertFalse(preview.has("preview_token"))
        assertEquals("activate", activation.getString("action"))
        assertEquals(
            "preview_restore",
            ScenarioFields.previewRequest(JSONObject().put("action", "restore")).getString("action"),
        )
    }

    @Test
    fun onlyExplicitActivationRequestsMayReconfigureRuntime() {
        for (action in listOf("list", "save", "delete", "preview", "preview_restore")) {
            assertFalse(ScenarioFields.activates(action))
        }
        assertTrue(ScenarioFields.activates("activate"))
        assertTrue(ScenarioFields.activates("restore"))
        assertEquals("SCENARIO_UNCONFIRMED", ScenarioFields.failure("private server details"))
        assertEquals("SCENARIO_ACCOUNT_UNAVAILABLE", ScenarioFields.failure("SCENARIO_ACCOUNT_UNAVAILABLE"))
    }
}
