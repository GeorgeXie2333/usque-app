package io.github.georgexie2333.usque

import org.json.JSONObject

/** Main-owner state; live capture and terminal failure retention are distinct. */
internal class ProtectedAccountHandoff {
    private var sequence = 0L
    private var requestedFailureRetention = false
    private var inheritedFailureRetention = false

    @Volatile
    var retained = false
        private set

    fun request(
        appliedProfile: String?,
        tunnelOwned: Boolean,
        mode: String?,
        runtimeProfile: String? = appliedProfile,
        phase: String = "connected",
    ): Long? {
        if (!tunnelOwned) return null
        if (!retained &&
            !(
                mode == "vpn" &&
                    phase in
                    setOf(
                        "preparing",
                        "connectingH3",
                        "connectingH2",
                        "connected",
                        "degraded",
                        "reconnecting",
                        "error",
                    ) &&
                    eligible(appliedProfile, runtimeProfile)
            )
        ) {
            return null
        }
        // Read the profile published with the owned FD, never a pending save
        // or the temporary capture indication shown during another handoff.
        requestedFailureRetention =
            inheritedFailureRetention || (appliedKillSwitch(appliedProfile) != false)
        return ++sequence
    }

    fun owns(token: Long): Boolean = token == sequence

    fun begin(token: Long): Boolean {
        if (!owns(token)) return false
        inheritedFailureRetention = requestedFailureRetention
        retained = true
        return true
    }

    fun inheritColdVpn(
        appliedProfile: String?,
        tunnelOwned: Boolean,
        targetTunnel: Boolean,
    ): Boolean {
        if (!targetTunnel) return false
        val token = request(appliedProfile, tunnelOwned, "vpn") ?: return false
        return begin(token)
    }

    fun retainAfterFailure(targetKillSwitch: Boolean): Boolean =
        targetKillSwitch || (retained && inheritedFailureRetention)

    fun stable() {
        retained = false
        inheritedFailureRetention = false
        requestedFailureRetention = false
    }

    fun disconnect() {
        sequence++
        stable()
    }

    private fun appliedKillSwitch(profile: String?): Boolean? =
        runCatching { JSONObject(requireNotNull(profile)).getBoolean("kill_switch") }.getOrNull()

    private fun eligible(
        profile: String?,
        runtimeProfile: String?,
    ): Boolean =
        profile?.let {
            runCatching {
                val source = JSONObject(it)
                val runtime = JSONObject(requireNotNull(runtimeProfile))
                VpnReconfigure.tunnelFrontendEnabled(source) && ChainProfileFields.proxy(source) &&
                    source.optString("id").isNotEmpty() && source.optString("id") == runtime.optString("id") &&
                    ChainProfileFields.selection(source) == ChainProfileFields.selection(runtime)
            }.getOrDefault(false)
        } == true
}
