package io.github.georgexie2333.usque

import org.json.JSONObject

internal object CurrentAccountProfile {
    fun read(catalogJson: String): String? {
        val catalog = JSONObject(catalogJson)
        val id = catalog.optString("active_profile_id")
        val profiles = catalog.optJSONArray("profiles") ?: return null
        if (id.isEmpty()) return null
        for (index in 0 until profiles.length()) {
            val profile = profiles.getJSONObject(index)
            if (profile.optString("id") == id) return profile.toString()
        }
        return null
    }
}
