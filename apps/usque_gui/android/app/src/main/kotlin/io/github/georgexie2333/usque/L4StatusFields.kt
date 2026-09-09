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
        if (value == null || value.length > 16384) return null
        val source = runCatching { JSONObject(value) }.getOrNull() ?: return null
        val result = linkedMapOf<String, Any>("connect_verified" to (source.opt("connect_verified") == true))
        for (key in counters) {
            val number = source.opt(key) as? Number ?: continue
            result[key] = number.toLong().coerceAtLeast(0)
        }
        source.optJSONObject("performance")?.let { result["performance"] = performance(it) }
        return result
    }

    private val performanceCounters =
        listOf(
            "h3_read_calls",
            "h3_read_bytes",
            "h3_empty_reads",
            "receive_pool_allocations",
            "receive_pool_hits",
            "receive_pool_evictions",
            "receive_pool_idle_bytes",
            "receive_pool_idle_high_watermark",
            "receive_pool_live_bytes",
            "receive_pool_live_high_watermark",
            "adapter_copied_bytes",
            "tcp_accepted_bytes",
            "tcp_write_calls",
            "tcp_partial_writes",
            "actor_wakeups",
            "actor_polls",
            "actor_no_progress_polls",
            "actor_no_progress_wakeups",
            "budget_wakeups",
            "tun_ingress_packets",
            "tun_ingress_bytes",
            "tun_egress_packets",
            "tun_egress_bytes",
            "tun_write_calls",
            "tun_write_would_block",
            "udp_receive_buffer_bytes",
            "udp_send_buffer_bytes",
            "tun_mtu",
            "tcp_preferred_sockets",
            "tcp_fallback_sockets",
            "tcp_buffer_bytes",
        )
    private val queueNames =
        listOf("tun_ingress_queue", "tun_egress_queue", "stack_ingress_queue", "stack_egress_queue")

    private fun count(value: Any?): Long? =
        when (value) {
            is Int -> value.toLong()
            is Long -> value
            else -> null
        }?.takeIf { it >= 0 }

    private fun wait(source: JSONObject?): Map<String, Any>? {
        source ?: return null
        val buckets = source.optJSONArray("buckets") ?: return null
        if (buckets.length() != 32) return null
        val values = (0 until 32).map { count(buckets.opt(it)) ?: return null }
        return linkedMapOf(
            "samples" to (count(source.opt("samples")) ?: return null),
            "sum_us" to (count(source.opt("sum_us")) ?: return null),
            "max_us" to (count(source.opt("max_us")) ?: return null),
            "buckets" to values,
        )
    }

    private fun performance(source: JSONObject): Map<String, Any> {
        val result = linkedMapOf<String, Any>()
        for (key in performanceCounters) {
            count(source.opt(key))?.let { result[key] = it }
        }
        if (source.opt("udp_buffer_source") == "getsockopt_raw") result["udp_buffer_source"] = "getsockopt_raw"
        if (source.opt("tun_mtu_source") == "applied_profile") result["tun_mtu_source"] = "applied_profile"
        for (key in listOf("command_wait", "tun_write_wait")) {
            wait(source.optJSONObject(key))?.let { result[key] = it }
        }
        for (key in queueNames) {
            val queue = source.optJSONObject(key) ?: continue
            val values = linkedMapOf<String, Any>()
            for (field in listOf("packets", "bytes", "high_water_packets", "high_water_bytes")) {
                count(queue.opt(field))?.let { values[field] = it }
            }
            if (values.size != 4) continue
            wait(queue.optJSONObject("wait"))?.let { values["wait"] = it }
            result[key] = values
        }
        return result
    }

    fun buildInfo(value: String?): JSONObject? {
        if (value == null || value.length > 1024) return null
        val source = runCatching { JSONObject(value) }.getOrNull() ?: return null
        val result = JSONObject()
        (source.opt("version") as? String)
            ?.takeIf {
                it.matches(Regex("[0-9A-Za-z.+-]{1,64}"))
            }?.let { result.put("version", it) }
        (source.opt("architecture") as? String)
            ?.takeIf {
                it in setOf("aarch64", "arm", "x86_64", "x86")
            }?.let { result.put("architecture", it) }
        (source.opt("debug_assertions") as? Boolean)?.let { result.put("debug_assertions", it) }
        return result
    }

    fun encode(value: JSONObject?): String? = decode(value?.toString())?.let { JSONObject(it).toString() }
}
