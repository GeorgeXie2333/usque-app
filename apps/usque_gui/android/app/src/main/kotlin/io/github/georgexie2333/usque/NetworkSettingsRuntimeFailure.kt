package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject

/** Evidence carried only by the application's owning token, never by an error string alone. */
internal sealed class NetworkSettingsRuntimeFailure {
    data object Unconfirmed : NetworkSettingsRuntimeFailure()

    data class Rejected(
        val profile: String,
    ) : NetworkSettingsRuntimeFailure()

    val uncertain: Boolean
        get() = this is Unconfirmed

    fun command(application: NetworkSettingsApplicationTracker.Application): JSONObject =
        JSONObject()
            .put("command", "failed")
            .put("operation_id", requireNotNull(application.operationId))
            .put("session_id", application.generation.toString())
            .apply {
                if (this@NetworkSettingsRuntimeFailure is Rejected) {
                    put("outcome", "rejected")
                    put("confirmed_profile", JSONObject(this@NetworkSettingsRuntimeFailure.profile))
                }
            }

    companion object {
        fun fromReply(
            application: NetworkSettingsApplicationTracker.Application,
            errorCode: String,
        ): NetworkSettingsRuntimeFailure {
            val profile = application.rejectedProfile
            return if (errorCode == "ROUTING_UPDATE_FAILED" && profile != null) Rejected(profile) else Unconfirmed
        }

        /** The caller reads this snapshot after reconfigure on the engine executor. */
        fun retainedProfile(
            nativeResult: Int,
            previousProfile: String?,
            confirmedProfile: String?,
            metadataRestored: Boolean,
            recoveryRestored: Boolean,
            nativeActive: Boolean,
            tunnelOwned: Boolean,
            nativeSnapshot: JSONObject?,
        ): String? {
            if (nativeResult != NativeEngine.RECONFIGURE_REJECTED || !metadataRestored || !recoveryRestored ||
                !nativeActive || previousProfile == null || previousProfile != confirmedProfile ||
                nativeSnapshot == null
            ) {
                return null
            }
            return runCatching {
                if (nativeSnapshot.getString("phase") !in setOf("connected", "degraded") ||
                    !nativeSnapshot.isNull("failure") ||
                    nativeSnapshot.getJSONObject("vpn_gate").getString("stage") !in setOf("disabled", "connected")
                ) {
                    return null
                }
                val profile = JSONObject(previousProfile)
                if (VpnReconfigure.tunnelFrontendEnabled(profile) && !tunnelOwned) return null
                if (nativeSnapshot.getString("data_plane") != profile.optString("data_plane", "connect_ip")) return null
                val frontends = profile.getJSONObject("frontends")
                val proxy = profile.getJSONObject("proxy")
                val expectedFrontends = mutableSetOf<String>()
                val expectedListeners = mutableSetOf<String>()
                for ((frontend, listeners) in listOf("socks5" to "socks5_listeners", "http" to "http_listeners")) {
                    if (frontends.getBoolean(frontend)) {
                        expectedFrontends += frontend
                        expectedListeners += strings(proxy.getJSONArray(listeners))
                    }
                }
                if (strings(nativeSnapshot.getJSONArray("active_frontends")) != expectedFrontends ||
                    strings(nativeSnapshot.getJSONArray("active_listeners")) != expectedListeners
                ) {
                    return null
                }
                previousProfile
            }.getOrNull()
        }

        private fun strings(values: JSONArray): Set<String> =
            (0 until values.length()).map { values.getString(it) }.toSet()
    }
}
