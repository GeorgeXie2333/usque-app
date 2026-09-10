package io.github.georgexie2333.usque

import org.json.JSONObject

/** Allowlisted counters only: never bridge arbitrary native JSON to Flutter. */
internal object L4StatusFields {
    const val MAX_JSON_BYTES = 96 * 1024
    private const val MAX_BUILD_INFO_JSON_BYTES = 1024
    private const val MAX_RECEIVE_HISTORY = 120

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
        if (value == null || exceedsJsonByteLimit(value, MAX_JSON_BYTES)) return null
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
        receive(source.optJSONObject("receive"))?.let { result["receive"] = it }
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
        if (value == null || exceedsJsonByteLimit(value, MAX_BUILD_INFO_JSON_BYTES)) return null
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
        // Read compatibility for historical comparison APKs, not build switches.
        (source.opt("network_experiment") as? String)
            ?.takeIf {
                it in
                    setOf(
                        "none",
                        "android_l4_portable_recv_only",
                        "android_l4_rcvbuf_control",
                        "android_l4_rcvbuf_2m",
                        "android_h3_rcvbuf_control",
                        "android_h3_rcvbuf_2m",
                    )
            }?.let { result.put("network_experiment", it) }
        return result
    }

    private fun exceedsJsonByteLimit(
        value: String,
        limit: Int,
    ): Boolean = value.length > limit || value.toByteArray(Charsets.UTF_8).size > limit

    fun encode(value: JSONObject?): String? = decode(value?.toString())?.let { JSONObject(it).toString() }

    /** Shared bounded allowlist for L4 and H3 socket exports. */
    fun receive(source: JSONObject?): Map<String, Any>? {
        source ?: return null
        val result = linkedMapOf<String, Any>()
        for (key in listOf(
            "requested_buffer_bytes",
            "buffer_target_bytes",
            "socket_drops_reported",
            "overflow_reports",
            "ancillary_errors",
            "recv_syscalls",
            "received_datagrams",
            "empty_recv_syscalls",
            "history_dropped",
        )) {
            count(source.opt(key))?.let { result[key] = it }
        }
        val states =
            mapOf(
                "buffer_request_status" to setOf("not_requested", "accepted", "rejected", "already_sufficient"),
                "overflow_monitoring" to setOf("enabled", "unavailable", "unavailable_backend"),
                "receive_backend" to setOf("portable", "recvmmsg"),
                "send_backend" to setOf("portable", "sendmmsg"),
            )
        for ((key, allowed) in states) {
            (source.opt(key) as? String)?.takeIf { it in allowed }?.let { result[key] = it }
        }
        val history = source.optJSONArray("history")
        if (history != null) {
            if (history.length() > MAX_RECEIVE_HISTORY) return null
            val items = mutableListOf<Map<String, Any>>()
            for (index in 0 until history.length()) {
                val item = history.optJSONObject(index) ?: return null
                val values = linkedMapOf<String, Any>()
                for (key in listOf(
                    "elapsed_ms",
                    "interval_ms",
                    "h3_read_bytes",
                    "tcp_accepted_bytes",
                    "tun_ingress_bytes",
                )) {
                    values[key] =
                        count(item.opt(key)) ?: return null
                }
                values["path_reset"] = item.opt("path_reset") as? Boolean ?: return null
                for (key in listOf("received_datagrams", "recv_syscalls", "socket_drops", "socket_drops_reported")) {
                    count(item.opt(key))?.let {
                        values[key] =
                            it
                    }
                }
                items.add(values)
            }
            result["history"] = items
        }
        return result
    }
}
