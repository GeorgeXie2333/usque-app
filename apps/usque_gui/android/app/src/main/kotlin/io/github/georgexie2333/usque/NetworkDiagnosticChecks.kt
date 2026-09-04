package io.github.georgexie2333.usque

import org.json.JSONObject

/** Fixed codes and numeric evidence only; never resolver names or addresses. */
internal object NetworkDiagnosticChecks {
    val standardIds =
        listOf(
            "quality.rtt",
            "quality.packet_loss",
            "quality.queue_pressure",
            "quality.pmtu",
            "transport.migration_capability",
            "dns.direct_encrypted_configuration",
            "dns.direct_encrypted_runtime_state",
        )
    val deepIds = listOf("transport.h3_path_validation_probe", "dns.direct_encrypted_reachability")

    fun result(
        id: String,
        status: String,
        summary: String,
        remediation: String = "none",
        evidence: List<String> = emptyList(),
    ): Map<String, Any?> =
        mapOf(
            "check_id" to id,
            "category" to if (id.startsWith("dns.")) "protection" else "transport",
            "status" to status,
            "severity" to
                if (status == "failed") {
                    "error"
                } else if (status == "warning") {
                    "warning"
                } else {
                    "info"
                },
            "summary_key" to summary,
            "remediation_key" to remediation,
            "sanitized_evidence" to evidence,
        )

    fun evaluate(
        id: String,
        snapshot: Map<String, Any?>,
        now: Long,
    ): Map<String, Any?> {
        fun unavailable() = result(id, "skipped", "nq_finding_unavailable")
        val q = snapshot["network_quality"] as? Map<*, *>
        val dns = q?.get("direct_dns") as? Map<*, *>
        val mode = snapshot["direct_dns_mode"] as? String ?: dns?.get("mode") as? String
        if (id == "dns.direct_encrypted_configuration") {
            return when {
                snapshot["direct_dns_configuration"] == "unsupported" -> {
                    result(id, "failed", "nq_finding_unsupported", "nq_profile")
                }

                mode == "physicalSystem" -> {
                    result(id, "skipped", "nq_finding_dns_system")
                }

                snapshot["direct_dns_configuration"] == "invalid" -> {
                    result(
                        id,
                        "failed",
                        "nq_finding_invalid_configuration",
                        "nq_profile",
                    )
                }

                mode in
                    setOf(
                        "doh",
                        "dot",
                    ) && snapshot["direct_dns_configuration"] == "valid" -> {
                    result(
                        id,
                        "passed",
                        "nq_finding_dns_custom_valid",
                        evidence = listOf("plaintext_fallback=0"),
                    )
                }

                else -> {
                    unavailable()
                }
            }
        }
        q ?: return unavailable()
        if (id in setOf("quality.packet_loss", "quality.pmtu", "transport.migration_capability") &&
            (snapshot["transport"] as? String)?.lowercase() !in setOf("h3", "http3", "http/3")
        ) {
            return unavailable()
        }
        if (q["connection_instance_id"] !is String) return unavailable()
        val sampled = (q["sampled_at_unix_ms"] as? Number)?.toLong() ?: return unavailable()
        if (sampled < now - 3_000 || sampled > now + 3_000) return result(id, "warning", "nq_finding_stale", "nq_retry")
        val metrics = q["metrics"] as? Map<*, *> ?: emptyMap<Any, Any>()

        fun metric(
            key: String,
            availability: String,
        ): Long? =
            if (metrics[availability] ==
                "available"
            ) {
                number(metrics, key)
            } else {
                null
            }

        fun measured(
            value: Long?,
            high: Long,
            key: String,
            summary: String,
        ): Map<String, Any?> {
            value ?: return unavailable()
            return result(
                id,
                if (value >=
                    high
                ) {
                    "warning"
                } else {
                    "passed"
                },
                if (value >=
                    high
                ) {
                    summary
                } else {
                    "nq_finding_healthy"
                },
                if (value >=
                    high
                ) {
                    "nq_network"
                } else {
                    "none"
                },
                listOf("$key=$value"),
            )
        }
        return when (id) {
            "quality.rtt" -> {
                measured(
                    metric("smoothed_rtt_milliseconds", "smoothed_rtt_availability"),
                    150,
                    "rtt_ms",
                    "nq_finding_rtt_high",
                )
            }

            "quality.packet_loss" -> {
                measured(
                    metric("interval_loss_basis_points", "interval_loss_availability"),
                    200,
                    "loss_basis_points",
                    "nq_finding_loss_high",
                )
            }

            "quality.queue_pressure" -> {
                var peak: Long? = null
                var drops = 0L
                for (queue in (q["queues"] as? List<*>).orEmpty().take(8)) {
                    val values = queue as? Map<*, *> ?: continue
                    if (values["availability"] != "available") continue
                    for (unit in listOf("items", "bytes")) {
                        val capacity = number(values, "capacity_$unit") ?: continue
                        val current = number(values, "current_$unit") ?: continue
                        if (capacity >
                            0
                        ) {
                            peak = maxOf(peak ?: 0, (current.toDouble() / capacity * 100).toLong().coerceIn(0, 100))
                        }
                    }
                    val count = number(values, "drop_items") ?: 0
                    drops = if (Long.MAX_VALUE - drops < count) Long.MAX_VALUE else drops + count
                }
                if (peak ==
                    null
                ) {
                    unavailable()
                } else {
                    result(
                        id,
                        if (peak >= 50 ||
                            drops > 0
                        ) {
                            "warning"
                        } else {
                            "passed"
                        },
                        if (peak >= 50 ||
                            drops > 0
                        ) {
                            "nq_finding_queue_pressure"
                        } else {
                            "nq_finding_healthy"
                        },
                        if (peak >= 50 ||
                            drops > 0
                        ) {
                            "nq_network"
                        } else {
                            "none"
                        },
                        listOf("queue_percent=$peak", "queue_drops=$drops"),
                    )
                }
            }

            "quality.pmtu" -> {
                val pmtu = q["pmtu"] as? Map<*, *>
                if (pmtu?.get("availability") != "available") {
                    unavailable()
                } else {
                    val degraded = pmtu["phase_code"] == "degraded"
                    result(
                        id,
                        if (degraded) "warning" else "passed",
                        if (degraded) "nq_finding_pmtu_degraded" else "nq_finding_healthy",
                        if (degraded) "nq_network" else "none",
                        listOfNotNull(
                            number(
                                pmtu,
                                "outer_pmtu_bytes",
                            )?.let {
                                "pmtu_bytes=$it"
                            },
                        ),
                    )
                }
            }

            "transport.migration_capability" -> {
                val transport = (snapshot["transport"] as? String)?.lowercase()
                val migration = q["migration"] as? Map<*, *>
                if (transport !in setOf("h3", "http3", "http/3") || migration == null) {
                    unavailable()
                } else {
                    val blocked =
                        migration["last_reason_code"] in
                            setOf("unsupported", "peer_cid_unavailable", "local_cid_unavailable")
                    result(
                        id,
                        if (blocked) "warning" else "passed",
                        if (blocked) "nq_finding_migration_reconnect" else "nq_finding_healthy",
                    )
                }
            }

            "dns.direct_encrypted_runtime_state" -> {
                when {
                    mode == "physicalSystem" -> result(id, "skipped", "nq_finding_dns_system")

                    mode !in setOf("doh", "dot") || dns == null -> unavailable()

                    dns["mode"] != mode -> result(id, "warning", "nq_finding_dns_changed", "nq_reconnect")

                    dns["phase_code"] == "ready" -> result(id, "passed", "nq_finding_dns_runtime")

                    dns["phase_code"] in
                        setOf(
                            "degraded",
                            "disabled",
                        )
                    -> result(id, "warning", "nq_finding_dns_degraded", "nq_profile")

                    else -> unavailable()
                }
            }

            else -> {
                unavailable()
            }
        }
    }

    fun probe(
        id: String,
        json: String?,
    ): Map<String, Any?> {
        val source = if (json != null && json.length <= 256) runCatching { JSONObject(json) }.getOrNull() else null
        return when (source?.optString("code")) {
            "passed" -> {
                result(
                    id,
                    "passed",
                    "nq_finding_probe_success",
                    evidence =
                        listOfNotNull(
                            (source.opt("milliseconds") as? Number)
                                ?.toLong()
                                ?.takeIf {
                                    it in
                                        0..4_000
                                }?.let { "probe_ms=$it" },
                        ),
                )
            }

            "failed" -> {
                result(id, "failed", "nq_finding_probe_failed", "nq_network")
            }

            "timeout" -> {
                result(id, "warning", "nq_finding_probe_timeout", "nq_retry")
            }

            "cancelled" -> {
                result(id, "cancelled", "nq_finding_probe_cancelled")
            }

            "network_changed" -> {
                result(id, "warning", "nq_finding_stale", "nq_retry")
            }

            else -> {
                result(id, "skipped", "nq_finding_probe_unsafe")
            }
        }
    }

    private fun number(
        source: Map<*, *>,
        key: String,
    ): Long? = (source[key] as? Number)?.toLong()?.takeIf { it >= 0 }
}
