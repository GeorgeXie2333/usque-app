package io.github.georgexie2333.usque

import org.json.JSONObject

/** Allowlisted counters only: never bridge arbitrary native JSON to Flutter. */
internal object L4StatusFields {
    fun mode(value: Any?): String? = (value as? String)?.takeIf { it == "connect_ip" || it == "l4_proxy" }

    private val counters =
        listOf(
            "sessions",
            "draining_sessions",
            "active_flows",
            "pending_flows",
            "connect_successes",
            "connect_failures",
            "connect_timeouts",
            "buffer_bytes",
            "budget_rejections",
            "send_backpressure",
            "receive_backpressure",
            "udp_rejected",
            "dns_successes",
            "dns_failures",
            "dns_timeouts",
            "migration_preserved_flows",
            "reconnect_terminated_flows",
            "tun_flows",
            "half_open_flows",
            "connect_latency_us",
            "unsupported_packets",
        )

    fun decode(value: String?): Map<String, Any>? {
        if (value == null || value.length > 8192) return null
        val source = runCatching { JSONObject(value) }.getOrNull() ?: return null
        val result = linkedMapOf<String, Any>("connect_verified" to (source.opt("connect_verified") == true))
        for (key in counters) {
            val number = source.opt(key) as? Number ?: continue
            result[key] = number.toLong().coerceAtLeast(0)
        }
        return result
    }

    fun encode(value: JSONObject?): String? = decode(value?.toString())?.let { JSONObject(it).toString() }
}
