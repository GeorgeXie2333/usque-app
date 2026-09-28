package io.github.georgexie2333.usque

import org.json.JSONObject

internal object BootConnectPolicy {
    fun profile(catalogJson: String): String? {
        val catalog = JSONObject(catalogJson)
        val active = catalog.optString("active_profile_id")
        if (active.isEmpty()) return null
        val profiles = catalog.optJSONArray("profiles") ?: return null
        for (index in 0 until profiles.length()) {
            val profile = profiles.getJSONObject(index)
            if (profile.optString("id") == active) {
                return profile.takeIf { it.optBoolean("auto_connect", false) }?.toString()
            }
        }
        return null
    }
}
