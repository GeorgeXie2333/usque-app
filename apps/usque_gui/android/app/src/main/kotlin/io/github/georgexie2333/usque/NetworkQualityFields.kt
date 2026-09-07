package io.github.georgexie2333.usque

import org.json.JSONObject

/** Bounded native/Messenger/Flutter bridge. Unknown keys and raw text never cross it. */
internal object NetworkQualityFields {
    const val MAX_JSON_BYTES = 16 * 1024
    private val availability = setOf("unknown", "available", "unsupported", "notReady", "stale")
    private val levels = setOf("unknown", "good", "fair", "poor", "limitedData", "disconnected")
    private val queueKinds =
        setOf(
            "tunToTransport",
            "proxyToTransport",
            "transportOutgoing",
            "h3DatagramSend",
            "h3WireSend",
            "transportToTun",
            "transportToProxy",
            "directDns",
        )
    private val pmtuPhases = setOf("unsupported", "unknown", "probing", "stable", "revalidating", "degraded")
    private val migrationPhases =
        setOf("idle", "preparing_socket", "probing", "validated", "promoting", "stable", "aborted")
    private val migrationReasons =
        setOf(
            "family_unavailable",
            "socket_protect_failed",
            "generation_changed_during_setup",
            "peer_cid_unavailable",
            "local_cid_unavailable",
            "path_probe_rejected",
            "path_validation_timeout",
            "superseded",
            "promotion_failed",
            "connection_closed",
            "unsupported",
        )
    private val dnsPhases = setOf("system", "connecting", "ready", "degraded", "disabled")
    private val dnsModes = setOf("unknown", "physicalSystem", "doh", "dot")
    private val dnsReasons = setOf("timeout", "query_failed", "network_changed", "unsupported")
    private val instanceId =
        Regex("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-4[0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$")
    private val metricNumbers =
        setOf(
            "latest_rtt_milliseconds",
            "smoothed_rtt_milliseconds",
            "minimum_rtt_milliseconds",
            "rtt_variance_milliseconds",
            "interval_loss_basis_points",
            "congestion_window_bytes",
            "bytes_in_flight",
            "send_rate_bits_per_second",
            "packets_lost",
            "bytes_lost",
            "tun_sink_drop_count",
            "quic_datagram_drop_count",
            "queue_oldest_age_milliseconds",
            "current_pmtu_bytes",
            "migration_attempt_count",
            "migration_success_count",
            "migration_failure_count",
            "last_migration_duration_milliseconds",
            "udp_send_syscall_count",
            "udp_recv_syscall_count",
            "udp_datagram_sent_count",
            "udp_datagram_received_count",
            "packet_buffer_pool_hit_count",
            "packet_buffer_pool_miss_count",
            "h2_flow_control_stall_count",
            "h2_flow_control_stall_total_milliseconds",
            "h2_flow_control_stall_max_milliseconds",
            "h2_stream_receive_window_bytes",
            "h2_connection_receive_window_bytes",
            "direct_dns_success_count",
            "direct_dns_failure_count",
            "direct_dns_timeout_count",
            "direct_dns_last_rtt_milliseconds",
            "pmtu_change_count",
            "pmtu_revalidation_failure_count",
            "pmtu_send_too_large_count",
        )
    private val metricAvailability =
        setOf(
            "latest_rtt_availability",
            "smoothed_rtt_availability",
            "minimum_rtt_availability",
            "rtt_variance_availability",
            "interval_loss_availability",
            "congestion_window_availability",
            "bytes_in_flight_availability",
            "send_rate_availability",
        )
    private val queueNumbers =
        setOf(
            "current_items",
            "capacity_items",
            "current_bytes",
            "capacity_bytes",
            "high_water_items",
            "high_water_bytes",
            "drop_items",
            "drop_bytes",
            "oldest_age_milliseconds",
            "enqueue_count",
            "dequeue_count",
        )

    fun encode(source: JSONObject?): String? {
        source ?: return null
        val result = JSONObject(sanitize(source)).toString()
        return result.takeIf { it.toByteArray(Charsets.UTF_8).size <= MAX_JSON_BYTES }
    }

    fun decode(value: String?): Map<String, Any?>? {
        if (value == null || value.length > MAX_JSON_BYTES ||
            value.toByteArray(Charsets.UTF_8).size > MAX_JSON_BYTES
        ) {
            return null
        }
        return runCatching { sanitize(JSONObject(value)) }.getOrNull()
    }

    fun capabilities(value: String?): Map<String, Boolean> {
        val source = if (value != null && value.length <= 1024) runCatching { JSONObject(value) }.getOrNull() else null
        return listOf("network_quality", "encrypted_direct_dns", "quic_migration", "automatic_pmtu").associateWith {
            source?.opt(it) ==
                true
        }
    }

    private fun sanitize(source: JSONObject): Map<String, Any?> {
        val metrics = numbers(source.optJSONObject("metrics"), metricNumbers).toMutableMap()
        for (key in metricAvailability) {
            metrics[key] =
                token(source.optJSONObject("metrics"), key, availability, "unknown")
        }
        val queues = source.optJSONArray("queues")
        val queueOutput =
            (0 until minOf(queues?.length() ?: 0, queueKinds.size))
                .mapNotNull { index ->
                    val value = queues?.optJSONObject(index) ?: return@mapNotNull null
                    val kind = token(value, "kind", queueKinds, "")
                    if (kind.isEmpty()) return@mapNotNull null
                    numbers(value, queueNumbers) +
                        mapOf(
                            "kind" to kind,
                            "availability" to token(value, "availability", availability, "unknown"),
                            "closed" to (value.opt("closed") == true),
                            "cancelled" to (value.opt("cancelled") == true),
                        )
                }.distinctBy { it["kind"] }
        val pmtu = source.optJSONObject("pmtu")
        val migration = source.optJSONObject("migration")
        val dns = source.optJSONObject("direct_dns")
        val samples = source.optJSONArray("samples")
        val sampleOutput =
            (0 until minOf(samples?.length() ?: 0, 16)).mapNotNull { index ->
                val sample = samples?.optJSONObject(index) ?: return@mapNotNull null
                val sequence = number(sample, "sequence") ?: return@mapNotNull null
                val at = number(sample, "sampled_at_unix_ms") ?: return@mapNotNull null
                val monotonic = number(sample, "monotonic_millis") ?: return@mapNotNull null
                if (sequence <= 0 || at <= 0 || at > 8_640_000_000_000_000L ||
                    monotonic > 8_640_000_000_000_000L
                ) {
                    return@mapNotNull null
                }
                numbers(sample, setOf("downloaded_bytes", "uploaded_bytes", "rtt_ms")) +
                    mapOf(
                        "sequence" to sequence,
                        "sampled_at_unix_ms" to at,
                        "monotonic_millis" to monotonic,
                        "loss_basis_points" to number(sample, "loss_basis_points")?.takeIf { it <= 10_000 },
                    )
            }
        return mapOf(
            "samples" to sampleOutput,
            "sampled_at_unix_ms" to number(source, "sampled_at_unix_ms"),
            "connection_instance_id" to (source.opt("connection_instance_id") as? String)?.takeIf(instanceId::matches),
            "level" to token(source, "level", levels, "unknown"),
            "metrics" to metrics,
            "queues" to queueOutput,
            "pmtu" to
                (
                    numbers(
                        pmtu,
                        setOf(
                            "outer_pmtu_bytes",
                            "effective_connect_ip_payload_bytes",
                            "change_count",
                            "revalidation_failure_count",
                            "send_too_large_count",
                        ),
                    ) +
                        mapOf(
                            "availability" to token(pmtu, "availability", availability, "unknown"),
                            "effective_payload_availability" to
                                token(pmtu, "effective_payload_availability", availability, "unknown"),
                            "phase_code" to token(pmtu, "phase_code", pmtuPhases, "unknown"),
                        )
                ),
            "migration" to
                (
                    numbers(
                        migration,
                        setOf("attempt_count", "success_count", "failure_count", "last_duration_milliseconds"),
                    ) +
                        mapOf(
                            "phase_code" to token(migration, "phase_code", migrationPhases, "idle"),
                            "last_reason_code" to token(migration, "last_reason_code", migrationReasons, ""),
                        )
                ),
            "direct_dns" to
                (
                    numbers(dns, setOf("success_count", "failure_count", "timeout_count", "last_rtt_milliseconds")) +
                        mapOf(
                            "mode" to token(dns, "mode", dnsModes, "unknown"),
                            "phase_code" to token(dns, "phase_code", dnsPhases, "disabled"),
                            "last_reason_code" to token(dns, "last_reason_code", dnsReasons, ""),
                        )
                ),
        )
    }

    private fun numbers(
        source: JSONObject?,
        keys: Set<String>,
    ): Map<String, Any?> =
        keys.associateWith {
            number(source, it)
        }

    private fun number(
        source: JSONObject?,
        key: String,
    ): Long? {
        val value = source?.opt(key) as? Number ?: return null
        val floating = value.toDouble()
        if (!floating.isFinite() || floating < 0 || floating % 1.0 != 0.0) return null
        return value.toLong().takeIf { it >= 0 }
    }

    private fun token(
        source: JSONObject?,
        key: String,
        allowed: Set<String>,
        fallback: String,
    ): String = (source?.opt(key) as? String)?.takeIf(allowed::contains) ?: fallback
}
