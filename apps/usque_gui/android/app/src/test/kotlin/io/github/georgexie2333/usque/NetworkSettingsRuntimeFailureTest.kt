package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NetworkSettingsRuntimeFailureTest {
    @Test
    fun confirmedRejectionReleasesTheApplicationAndCarriesTheOldProfileIntoItsFailureCommand() {
        val tracker = NetworkSettingsApplicationTracker()
        val token = tracker.begin(7)
        assertTrue(tracker.committed(token, 7, "rejected-save"))
        val retained = requireNotNull(retainedProfile())
        assertTrue(tracker.confirmRejection(token, 7, retained))
        val application = requireNotNull(tracker.runtimeReplied(token, 7))
        val failure = NetworkSettingsRuntimeFailure.fromReply(application, "ROUTING_UPDATE_FAILED")
        assertFalse(failure.uncertain)
        val command = failure.command(application)
        assertEquals("failed", command.getString("command"))
        assertEquals("rejected", command.getString("outcome"))
        assertEquals("rejected-save", command.getString("operation_id"))
        assertEquals("7", command.getString("session_id"))
        val confirmed = command.getJSONObject("confirmed_profile")
        assertEquals("account", confirmed.getString("id"))
        assertTrue(confirmed.getJSONObject("frontends").getBoolean("tunnel"))
        assertEquals(
            "[\"127.0.0.1:1080\",\"[::1]:1080\"]",
            confirmed.getJSONObject("proxy").getJSONArray("socks5_listeners").toString(),
        )
        assertTrue(NetworkSettingsRuntimeFailure.fromReply(application, "RECOVERY_UNAVAILABLE").uncertain)
        tracker.finish(token, 7)
        assertFalse(tracker.busy)

        val next = tracker.begin(7)
        assertTrue(tracker.committed(next, 7, "retry"))
        assertNull(tracker.runtimeReplied(token, 7))
        val retry = requireNotNull(tracker.runtimeReplied(next, 7))
        val unconfirmed = NetworkSettingsRuntimeFailure.fromReply(retry, "ROUTING_UPDATE_FAILED")
        assertTrue(unconfirmed.uncertain)
        assertFalse(unconfirmed.command(retry).has("confirmed_profile"))
        assertFalse(unconfirmed.command(retry).has("outcome"))
    }

    @Test
    fun staleTokensGenerationsAndRepliesCannotSupplyConfirmationForAnotherApplication() {
        val tracker = NetworkSettingsApplicationTracker()
        val token = tracker.begin(7)
        assertFalse(tracker.confirmRejection(token, 7, profile))
        assertTrue(tracker.committed(token, 7, "old"))
        assertFalse(tracker.confirmRejection(token + 1, 7, profile))
        assertFalse(tracker.confirmRejection(token, 8, profile))
        assertNull(tracker.runtimeReplied(token + 1, 7))
        assertNull(tracker.runtimeReplied(token, 8))
        assertTrue(tracker.confirmRejection(token, 7, profile))
        assertTrue(tracker.migrateSession(token, 7, 8))
        val migrated = requireNotNull(tracker.runtimeReplied(token, 8))
        assertTrue(NetworkSettingsRuntimeFailure.fromReply(migrated, "ROUTING_UPDATE_FAILED").uncertain)
        assertFalse(tracker.confirmRejection(token, 8, profile))
        assertNull(tracker.runtimeReplied(token, 8))
        tracker.cancel()
        val next = tracker.begin(8)
        assertTrue(tracker.committed(next, 8, "new"))
        assertFalse(tracker.confirmRejection(token, 8, profile))
        assertNull(tracker.runtimeReplied(token, 8))
        assertTrue(tracker.owns(next, 8))
    }

    @Test
    fun missingOrFailedRollbackAndRuntimeEvidenceRemainsUnconfirmed() {
        assertNull(retainedProfile(nativeResult = NativeEngine.ERROR_PLATFORM_FAILURE))
        assertNull(retainedProfile(metadataRestored = false))
        assertNull(retainedProfile(recoveryRestored = false))
        assertNull(retainedProfile(nativeActive = false))
        assertNull(retainedProfile(tunnelOwned = false))
        assertNull(retainedProfile(previousProfile = null))
        assertNull(retainedProfile(confirmedProfile = null))
        assertNull(retainedProfile(confirmedProfile = JSONObject(profile).put("id", "different").toString()))
        assertNull(retainedProfile(nativeSnapshot = null))
        assertNull(retainedProfile(nativeSnapshot = JSONObject()))
        for (phase in listOf("reconnecting", "error", "disconnected")) {
            assertNull(retainedProfile(nativeSnapshot = snapshot().put("phase", phase)))
        }
        for (stage in listOf("reconnecting", "configuring_network", "error")) {
            assertNull(retainedProfile(nativeSnapshot = snapshot().put("vpn_gate", JSONObject().put("stage", stage))))
        }
        assertNull(retainedProfile(nativeSnapshot = snapshot().put("failure", JSONObject().put("code", "failed"))))
        assertNull(retainedProfile(nativeSnapshot = snapshot().put("active_frontends", JSONArray())))
        assertNull(
            retainedProfile(nativeSnapshot = snapshot().put("active_listeners", JSONArray().put("127.0.0.1:9090"))),
        )
        assertNull(retainedProfile(nativeSnapshot = snapshot().put("data_plane", "l4_proxy")))
    }

    @Test
    fun healthyDegradedAndProxyOnlySessionsCanRetainTheirConfirmedProfile() {
        assertEquals(profile, retainedProfile())
        assertEquals(profile, retainedProfile(nativeSnapshot = snapshot().put("phase", "degraded")))
        val proxy =
            JSONObject(profile)
                .apply {
                    getJSONObject("frontends").put("tunnel", false)
                }.toString()
        assertEquals(proxy, retainedProfile(previousProfile = proxy, confirmedProfile = proxy, tunnelOwned = false))
    }

    private fun retainedProfile(
        nativeResult: Int = NativeEngine.RECONFIGURE_REJECTED,
        previousProfile: String? = profile,
        confirmedProfile: String? = profile,
        metadataRestored: Boolean = true,
        recoveryRestored: Boolean = true,
        nativeActive: Boolean = true,
        tunnelOwned: Boolean = true,
        nativeSnapshot: JSONObject? = snapshot(),
    ): String? =
        NetworkSettingsRuntimeFailure.retainedProfile(
            nativeResult,
            previousProfile,
            confirmedProfile,
            metadataRestored,
            recoveryRestored,
            nativeActive,
            tunnelOwned,
            nativeSnapshot,
        )

    private fun snapshot(): JSONObject =
        JSONObject()
            .put("phase", "connected")
            .put("vpn_gate", JSONObject().put("stage", "disabled"))
            .put("data_plane", "connect_ip")
            .put("active_frontends", JSONArray().put("socks5"))
            .put("active_listeners", JSONArray().put("[::1]:1080").put("127.0.0.1:1080"))

    private val profile =
        """{"id":"account","data_plane":"connect_ip","mode":"vpn",
            |"frontends":{"tunnel":true,"socks5":true,"http":false},
            |"proxy":{"socks5_listeners":["127.0.0.1:1080","[::1]:1080"],"http_listeners":[]}}
        """.trimMargin()
}
