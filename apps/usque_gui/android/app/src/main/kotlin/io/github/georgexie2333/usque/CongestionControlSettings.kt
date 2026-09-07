package io.github.georgexie2333.usque

import org.json.JSONObject

/** Configuration names and session binding only; never changes system TCP. */
internal object CongestionControlSettings {
    val algorithms = listOf("cubic", "reno", "bbr", "bbr3")

    fun token(value: Any?): String? = (value as? String)?.takeIf { it in algorithms }

    fun capabilities(value: String?): List<String> {
        if (value == null || value.length > 1024) return emptyList()
        val source = runCatching { JSONObject(value).optJSONArray("h3_congestion_control_algorithms") }.getOrNull()
        return (0 until minOf(source?.length() ?: 0, 32)).mapNotNull { token(source?.opt(it)) }.distinct()
    }

    fun forCurrentSession(
        desiredJson: String,
        activeJson: String?,
    ): String {
        val desired = JSONObject(desiredJson)
        val active = activeJson?.let(::JSONObject)
        if (active != null && active.optString("id") == desired.optString("id")) {
            desired.put("congestion_control", configuredAlgorithm(active))
        }
        return desired.toString()
    }

    fun forNewSession(
        recoveryJson: String,
        desiredJson: String?,
    ): String {
        val recovery = JSONObject(recoveryJson)
        val desired = desiredJson?.let(::JSONObject)
        if (desired != null && desired.optString("id") == recovery.optString("id")) {
            recovery.put("congestion_control", configuredAlgorithm(desired))
        }
        return recovery.toString()
    }

    fun fromCatalog(
        profileJson: String,
        catalogJson: String,
    ): String {
        val profileId = JSONObject(profileJson).getString("id")
        val profiles = JSONObject(catalogJson).getJSONArray("profiles")
        require(profiles.length() <= 128)
        for (index in 0 until profiles.length()) {
            val profile = profiles.getJSONObject(index)
            if (profile.optString("id") == profileId) {
                return forNewSession(profileJson, profile.toString())
            }
        }
        error("The selected profile no longer exists")
    }

    private fun configuredAlgorithm(profile: JSONObject): String {
        if (!profile.has("congestion_control")) return "cubic"
        return requireNotNull(token(profile.opt("congestion_control")))
    }
}
