package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ScenarioIdentityAuthorityTest {
    private fun profile(): JSONObject =
        JSONObject()
            .put("id", "account")
            .put("endpoint_v4", "162.159.198.2")
            .put("endpoint_v6", "2606:4700:103::2")
            .put("endpoint_port", 443)
            .put("sni", "speed.cloudflare.com")

    private fun catalog(
        profile: JSONObject = profile(),
        state: IdentityReplacementState = IdentityReplacementState.None,
    ): JSONObject =
        JSONObject()
            .put("profiles", JSONArray().put(profile))
            .put(
                "pending_identity_replacements",
                JSONArray().apply { if (state != IdentityReplacementState.None) put("account") },
            ).put(
                "armed_identity_replacements",
                JSONArray().apply { if (state == IdentityReplacementState.Armed) put("account") },
            )

    private fun authority(
        profile: JSONObject = profile(),
        state: IdentityReplacementState = IdentityReplacementState.None,
    ): IdentityReplacementAuthority =
        requireNotNull(IdentityReplacementAuthority.fromCatalog("account", catalog(profile, state).toString()))

    @Test
    fun scenePreviewAcceptsNewEndpointsWithoutRelaxingStartupAuthority() {
        val authority = authority()
        for ((field, value) in listOf(
            "endpoint_v4" to "192.0.2.10",
            "endpoint_v6" to "2001:db8::10",
            "endpoint_port" to 8443,
            "sni" to "edge.example.com",
        )) {
            val target = profile().put(field, value)
            assertFalse(authority.matches(target.toString()))
            // Normal startup still rejects these uncommitted endpoints.
            assertNull(
                authority.loadSecret(
                    target.toString(),
                    null,
                    null,
                    true,
                    readCurrent = { error("Uncommitted startup must not read credentials") },
                    discardRollback = { error("Read-only startup must not mutate credentials") },
                ),
            )
            val secret = byteArrayOf(4, 2)
            val id =
                ScenarioFields.validateIdentity(
                    JSONObject().put("preview_target", target),
                    loadProfile = { authority.profileJson },
                    loadSecret = { _, persisted ->
                        authority.loadSecret(
                            persisted,
                            null,
                            null,
                            true,
                            readCurrent = { secret },
                            discardRollback = { error("Scene preview must not repair storage") },
                        )
                    },
                    inspect = { assertArrayEquals(byteArrayOf(4, 2), it) },
                )
            assertEquals("account", id)
            assertTrue(secret.all { it == 0.toByte() })
            assertEquals(value, target.get(field))
        }
    }

    @Test
    fun scenePreflightUsesLivePreparingOrArmedIdentityAndClearsConsumerBuffers() {
        for (state in IdentityReplacementState.entries) {
            for (reject in listOf(false, true)) {
                val authority = authority(state = state)
                assertEquals(state, authority.state)
                val current = byteArrayOf(1, 2)
                val rollback = IdentityReplacementRollbackCodec.encode(byteArrayOf(3, 4), byteArrayOf(8), null)
                var returned: ByteArray? = null
                var reads = 0
                val result =
                    runCatching {
                        ScenarioFields.validateIdentity(
                            JSONObject().put("preview_target", profile().put("endpoint_port", 8443)),
                            loadProfile = { authority.profileJson },
                            loadSecret = { _, persisted ->
                                try {
                                    authority
                                        .loadSecret(
                                            persisted,
                                            current,
                                            rollback,
                                            true,
                                            readCurrent = {
                                                reads++
                                                byteArrayOf(5, 6)
                                            },
                                            discardRollback = { error("Read-only preflight cannot delete rollback") },
                                        ).also { returned = it }
                                } finally {
                                    current.fill(0)
                                    rollback.fill(0)
                                }
                            },
                            inspect = {
                                assertArrayEquals(
                                    when (state) {
                                        IdentityReplacementState.None -> byteArrayOf(5, 6)
                                        IdentityReplacementState.Preparing -> byteArrayOf(1, 2)
                                        IdentityReplacementState.Armed -> byteArrayOf(3, 4)
                                    },
                                    it,
                                )
                                if (reject) error("invalid private identity")
                            },
                        )
                    }
                assertEquals(if (state == IdentityReplacementState.None) 1 else 0, reads)
                assertTrue(current.all { it == 0.toByte() })
                assertTrue(rollback.all { it == 0.toByte() })
                assertTrue(requireNotNull(returned).all { it == 0.toByte() })
                if (reject) {
                    assertEquals("SCENARIO_ACCOUNT_UNAVAILABLE", result.exceptionOrNull()?.message)
                } else {
                    assertEquals("account", result.getOrThrow())
                }
            }
        }
    }

    @Test
    fun unreadyZeroTrustAuthorityRemainsUnavailableDespiteValidSceneEndpoints() {
        val authority = authority(profile().put("zero_trust_endpoint_ready", false))
        val result =
            runCatching {
                ScenarioFields.validateIdentity(
                    JSONObject().put("preview_target", profile().put("endpoint_port", 8443)),
                    loadProfile = { authority.profileJson },
                    loadSecret = { _, persisted ->
                        authority.loadSecret(
                            persisted,
                            null,
                            null,
                            true,
                            readCurrent = { error("Unready authority cannot load an identity") },
                            discardRollback = { error("Read-only") },
                        )
                    },
                    inspect = { error("Unready authority cannot inspect credentials") },
                )
            }
        assertEquals("SCENARIO_ACCOUNT_UNAVAILABLE", result.exceptionOrNull()?.message)
    }

    @Test
    fun missingOrWrongAccountAuthorityCannotReachTheSecretStore() {
        for (persisted in listOf(null, profile().put("id", "other-account").toString())) {
            var read = false
            val result =
                runCatching {
                    ScenarioFields.validateIdentity(
                        JSONObject().put("preview_target", profile()),
                        loadProfile = { persisted },
                        loadSecret = { _, _ ->
                            read = true
                            byteArrayOf(1)
                        },
                        inspect = { error("Missing authority cannot inspect") },
                    )
                }
            assertFalse(read)
            assertEquals("SCENARIO_ACCOUNT_UNAVAILABLE", result.exceptionOrNull()?.message)
        }
        assertNull(IdentityReplacementAuthority.fromCatalog("missing", catalog().toString()))
        val missingArmedState =
            catalog(state = IdentityReplacementState.Armed).apply {
                remove("armed_identity_replacements")
            }
        assertNull(IdentityReplacementAuthority.fromCatalog("account", missingArmedState.toString()))
    }

    @Test
    fun armedAuthorityWithMissingOrCorruptRollbackNeverUsesNewLiveCredentials() {
        val authority = authority(state = IdentityReplacementState.Armed)
        for (rollback in listOf(null, byteArrayOf(99))) {
            var liveRead = false
            val result =
                runCatching {
                    ScenarioFields.validateIdentity(
                        JSONObject().put("preview_target", profile().put("endpoint_port", 8443)),
                        loadProfile = { authority.profileJson },
                        loadSecret = { _, persisted ->
                            authority.loadSecret(
                                persisted,
                                byteArrayOf(9),
                                rollback,
                                true,
                                readCurrent = {
                                    liveRead = true
                                    byteArrayOf(9)
                                },
                                discardRollback = { error("Read-only") },
                            )
                        },
                        inspect = { error("Missing rollback cannot inspect") },
                    )
                }
            assertFalse(liveRead)
            assertEquals("SCENARIO_ACCOUNT_UNAVAILABLE", result.exceptionOrNull()?.message)
        }
    }

    @Test
    fun authorityChangedBetweenCatalogAndSecretReadsRejectsStaleProfile() {
        val before = authority()
        val after = authority(profile().put("endpoint_port", 444))
        var read = false
        val result =
            runCatching {
                ScenarioFields.validateIdentity(
                    JSONObject().put("preview_target", profile().put("endpoint_port", 8443)),
                    loadProfile = { before.profileJson },
                    loadSecret = { _, persisted ->
                        after.loadSecret(
                            persisted,
                            null,
                            null,
                            true,
                            readCurrent = {
                                read = true
                                byteArrayOf(1)
                            },
                            discardRollback = { error("Read-only") },
                        )
                    },
                    inspect = { error("Stale authority cannot inspect") },
                )
            }
        assertFalse(read)
        assertEquals("SCENARIO_ACCOUNT_UNAVAILABLE", result.exceptionOrNull()?.message)
    }

    @Test
    fun ordinaryStartupRetainsStaleRollbackCleanupWhilePreflightIsReadOnly() {
        val authority = authority()
        for (readOnly in listOf(true, false)) {
            var discarded = false
            val secret =
                authority.loadSecret(
                    authority.profileJson,
                    null,
                    byteArrayOf(99),
                    readOnly,
                    readCurrent = { byteArrayOf(7) },
                    discardRollback = { discarded = true },
                )
            assertArrayEquals(byteArrayOf(7), secret)
            secret?.fill(0)
            assertEquals(!readOnly, discarded)
        }
    }
}
