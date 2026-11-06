package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject

internal object NetworkSettingsFields {
    /** A UI-process configuration read is not an observation of the VPN owner. */
    fun persistedOnly(value: String): Map<String, Any?> {
        require(value.length <= 2 * 1024 * 1024)
        val source = JSONObject(value)
        val projected = JSONObject().put("runtime_observed", false)
        for (key in listOf("stored_profile", "shared_network_profile")) {
            source.optJSONObject(key)?.let { projected.put(key, it) }
        }
        return objectMap(projected)
    }

    fun decode(value: String): Map<String, Any?> {
        require(value.length <= 2 * 1024 * 1024)
        val source = JSONObject(value)
        require(source.getString("source_epoch").isNotBlank())
        require(source.getLong("sequence") >= 0)
        source.remove("target")
        return objectMap(source)
    }

    fun savedProfile(
        requested: String,
        catalog: String,
    ): String {
        val id = JSONObject(requested).getString("id")
        val profiles = JSONObject(catalog).getJSONArray("profiles")
        for (index in 0 until profiles.length()) {
            val profile = profiles.getJSONObject(index)
            if (profile.getString("id") == id) return profile.toString()
        }
        error("The saved account is unavailable")
    }

    private fun objectMap(source: JSONObject): Map<String, Any?> =
        source.keys().asSequence().associateWith { convert(source.get(it)) }

    private fun convert(value: Any?): Any? =
        when (value) {
            null, JSONObject.NULL -> null
            is JSONObject -> objectMap(value)
            is JSONArray -> (0 until value.length()).map { convert(value.get(it)) }
            else -> value
        }
}
