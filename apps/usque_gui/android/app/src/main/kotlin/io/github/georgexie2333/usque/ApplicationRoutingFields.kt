package io.github.georgexie2333.usque

import org.json.JSONObject

/** Rust validates targets; these flags only determine platform DNS setup. */
internal data class ApplicationRoutingFields(
    val needsDns: Boolean,
    val directDns: Boolean,
) {
    companion object {
        fun parse(profile: JSONObject): ApplicationRoutingFields {
            val routing = profile.optJSONObject("routing") ?: return ApplicationRoutingFields(false, false)
            var needsDns = routing.optBoolean("ads_enabled", false)
            var directDns = false
            val rules = routing.optJSONArray("rules")
            require((rules?.length() ?: 0) <= 512) { "ROUTING_RULE_LIMIT" }
            for (index in 0 until (rules?.length() ?: 0)) {
                val rule = requireNotNull(rules).getJSONObject(index)
                val kind = rule.getString("kind")
                val action = rule.getString("action")
                require(kind in setOf("domain", "cidr")) { "ROUTING_RULE_INVALID" }
                require(action in setOf("direct", "reject", "proxy")) { "ROUTING_RULE_INVALID" }
                if (kind == "domain") {
                    needsDns = true
                    if (action == "direct") directDns = true
                }
            }
            return ApplicationRoutingFields(needsDns, directDns)
        }
    }
}
