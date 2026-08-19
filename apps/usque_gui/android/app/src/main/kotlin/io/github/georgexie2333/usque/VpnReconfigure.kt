package io.github.georgexie2333.usque

import android.os.ParcelFileDescriptor
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicReference

/** TUN and recovery follow `frontends.tunnel`; `mode` is derived. */
internal object VpnReconfigure {
    fun tunnelFrontendEnabled(profileJson: String): Boolean = tunnelFrontendEnabled(JSONObject(profileJson))

    fun tunnelFrontendEnabled(source: JSONObject): Boolean {
        val frontends = source.optJSONObject("frontends")
        if (frontends != null && frontends.has("tunnel")) {
            return frontends.getBoolean("tunnel")
        }
        return source.optString("mode") == "vpn"
    }

    fun canonicalMode(tunnelEnabled: Boolean): String = if (tunnelEnabled) "vpn" else "socks5"

    fun canonicalizeProfileArguments(profile: Map<*, *>): Map<Any?, Any?> {
        val frontends = profile["frontends"] as? Map<*, *>
        val legacyMode = profile["mode"] as? String
        val tunnelEnabled =
            when (val tunnel = frontends?.get("tunnel")) {
                is Boolean -> tunnel
                else -> legacyMode == "vpn"
            }
        val normalized = LinkedHashMap<Any?, Any?>()
        profile.forEach { (key, value) -> normalized[key] = value }
        normalized["mode"] = canonicalMode(tunnelEnabled)
        return normalized
    }

    fun shouldTearDownTun(
        what: Int,
        profileJson: String?,
    ): Boolean {
        if (what != UsqueVpnService.MSG_RECONFIGURE || profileJson.isNullOrEmpty()) {
            return false
        }
        return !tunnelFrontendEnabled(profileJson)
    }

    fun applyNativeOk(
        what: Int,
        profileJson: String?,
        tunnel: AtomicReference<ParcelFileDescriptor?>,
        lastTunIdentity: AtomicReference<TunIdentity?>,
        closeQuietly: (ParcelFileDescriptor?) -> Unit,
    ) {
        if (!shouldTearDownTun(what, profileJson)) {
            return
        }
        lastTunIdentity.set(null)
        closeQuietly(tunnel.getAndSet(null))
    }
}
