package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject

internal object ChainProfileFields {
    private fun settings(profile: JSONObject): JSONObject? =
        profile.optJSONObject("chain_exit") ?: profile.optJSONObject("vpn_gate")

    fun enabled(profile: JSONObject): Boolean = settings(profile)?.optBoolean("enabled") == true

    fun custom(profile: JSONObject): Boolean =
        profile.optJSONObject("chain_exit")?.let { it.optString("source") != "vpn_gate" } == true

    fun selection(profile: JSONObject): String? {
        val chain = profile.optJSONObject("chain_exit")
        return if (chain != null && chain.optString("source") != "vpn_gate") {
            listOf(
                chain.optBoolean("enabled"),
                chain.optString("source"),
                chain.optString("profile_id"),
                chain.optString("revision"),
                chain.optJSONObject("endpoint_override")?.optString("host"),
                chain.optJSONObject("endpoint_override")?.optInt("port"),
            ).joinToString("|")
        } else {
            val gate = profile.optJSONObject("vpn_gate") ?: return null
            val selected = gate.optJSONObject("selection")
            listOf(
                enabled(profile),
                "vpn_gate",
                selected?.optString("server_id"),
                selected?.optString("config_sha256"),
            ).joinToString("|")
        }
    }

    private val summaryKeys =
        setOf(
            "id",
            "revision",
            "edit_revision",
            "name",
            "protocol",
            "source",
            "endpoint",
            "candidates",
            "remote_random",
            "address_family",
            "addresses",
            "dns_servers",
            "allowed_ips",
            "mtu",
            "requires_auth",
            "requires_key_password",
        )

    fun summary(source: JSONObject?): Map<String, Any?>? =
        source?.let { value ->
            summaryKeys.associateWith { key ->
                if (key == "candidates") {
                    value.optJSONArray(key)?.let { candidates ->
                        require(candidates.length() <= 16)
                        List(candidates.length()) { index ->
                            val candidate = candidates.getJSONObject(index)
                            val endpoint = candidate.getJSONObject("endpoint")
                            mapOf(
                                "endpoint" to
                                    mapOf("host" to endpoint.optString("host"), "port" to endpoint.optInt("port")),
                                "ipv6" to primitive(candidate.opt("ipv6")),
                            )
                        }
                    }
                } else if (key == "endpoint") {
                    value.optJSONObject(key)?.let {
                        mapOf(
                            "host" to it.optString("host"),
                            "port" to it.optInt("port"),
                        )
                    }
                } else {
                    primitive(value.opt(key))
                }
            }
        }

    fun response(raw: String): Map<String, Any?> {
        require(raw.length <= 1024 * 1024)
        val source = JSONObject(raw)
        val profiles = source.optJSONArray("profiles") ?: JSONArray()
        require(profiles.length() <= 128)
        return mapOf(
            "profiles" to List(profiles.length()) { summary(profiles.optJSONObject(it)) },
            "preview" to summary(source.optJSONObject("preview")),
            "error" to
                source.optJSONObject("error")?.let {
                    mapOf(
                        "line" to it.optInt("line"),
                        "field" to it.optString("field"),
                        "reason" to it.optString("reason"),
                    )
                },
        )
    }

    private fun primitive(value: Any?): Any? =
        when (value) {
            null, JSONObject.NULL -> null
            is JSONArray -> List(value.length()) { primitive(value.opt(it)) }
            is String, is Number, is Boolean -> value
            else -> null
        }
}
