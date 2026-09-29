package io.github.georgexie2333.usque

/** Public metadata accepts fixed tokens and unsigned integers, never arbitrary text. */
internal object DiagnosticMetadata {
    private val sources = setOf("unknown", "config", "runtime", "platform", "active_probe", "frontend")
    private val availability = setOf("observed", "inferred", "unavailable", "stale", "not_applicable")
    private val instanceId = Regex("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
    val transportStages =
        setOf(
            "endpoint_resolution",
            "socket_creation",
            "socket_protection",
            "socket_connect",
            "tls_handshake",
            "quic_handshake",
            "masque_connect",
            "peer_settings",
            "address_assignment",
            "tunnel_startup",
            "packet_send",
            "packet_receive",
            "dns_apply",
            "route_apply",
            "kill_switch_apply",
            "platform_recovery",
            "diagnostics",
        )

    fun unsigned(value: Any?): Long? =
        when (value) {
            is Byte, is Short, is Int, is Long -> (value as Number).toLong().takeIf { it >= 0 }
            else -> null
        }

    fun runtimeId(value: Any?): String? = (value as? String)?.takeIf(instanceId::matches)

    fun connectionId(snapshot: Map<String, Any?>): String? =
        runtimeId(snapshot["connection_instance_id"])
            ?: runtimeId((snapshot["network_quality"] as? Map<*, *>)?.get("connection_instance_id"))

    fun captureSummary(
        snapshot: Map<String, Any?>,
        timeline: Map<String, Any?>,
    ): Map<String, Any?> {
        val current = connectionId(snapshot)
        val timelineId = runtimeId(timeline["connection_instance_id"])
        val state =
            if (current == null ||
                timelineId == null
            ) {
                "unavailable"
            } else if (current == timelineId) {
                "observed"
            } else {
                "stale"
            }
        return mapOf(
            "connection_instance_id" to current,
            "connection_generation" to unsigned(snapshot["connection_generation"]),
            "network_generation" to unsigned(snapshot["network_generation"]),
            "timeline_connection_instance_id" to timelineId,
            "timeline_session_generation" to unsigned(timeline["session_generation"]),
            "timeline_scope_availability" to state,
            "omitted_mismatched_event_count" to
                if (state == "stale") (timeline["events"] as? List<*>)?.size?.toLong() else null,
        ).filterValues { it != null }
    }

    fun observation(value: Any?): Map<String, Any?>? {
        val raw = value as? Map<*, *> ?: return null
        val source = (raw["source"] as? String)?.takeIf(sources::contains) ?: return null
        val state = (raw["availability"] as? String)?.takeIf(availability::contains) ?: return null
        return mapOf(
            "source" to source,
            "availability" to state,
            "age_milliseconds" to unsigned(raw["age_milliseconds"]),
            "network_generation" to unsigned(raw["network_generation"]),
            "connection_instance_id" to runtimeId(raw["connection_instance_id"]),
        ).filterValues { it != null }
    }

    fun evidence(value: Any?): Map<String, Any?>? {
        val raw = value as? Map<*, *> ?: return null
        val key = raw["key"] as? String ?: return null
        val number = unsigned(raw["number"])
        val token = raw["token"] as? String
        if (number != null && token == null &&
            key in DiagnosticsContract.evidenceKeys
        ) {
            return mapOf("key" to key, "number" to number)
        }
        if (raw["number"] == null && key == "fact" &&
            token in DiagnosticsContract.evidenceTokens
        ) {
            return mapOf("key" to "fact", "token" to token)
        }
        return null
    }

    fun fromLegacy(value: String): Map<String, Any?>? {
        val legacyToken =
            when (value) {
                "network=present" -> "network_present"
                "network=absent" -> "network_absent"
                "kill_switch=active" -> "kill_switch_active"
                "kill_switch=inactive" -> "kill_switch_inactive"
                "kill_switch=notApplicable" -> "kill_switch_not_applicable"
                "kill_switch=unknown" -> "kill_switch_unknown"
                else -> value
            }
        if (legacyToken in DiagnosticsContract.evidenceTokens) return mapOf("key" to "fact", "token" to legacyToken)
        val split = value.split('=', limit = 2)
        if (split.size != 2 || split[1].isEmpty() || split[1].any { it !in '0'..'9' }) return null
        val key = if (split[0] == "generation") "network_generation" else split[0]
        return evidence(mapOf("key" to key, "number" to split[1].toLongOrNull()))
    }

    fun source(checkId: String): String =
        when {
            checkId in NetworkDiagnosticChecks.deepIds -> "active_probe"

            checkId == "engine.configuration" || checkId == "dns.direct_encrypted_configuration" -> "config"

            checkId.startsWith("physical.") || checkId.startsWith("protection.") ||
                checkId in setOf("tunnel.routes", "tunnel.dns", "tunnel.address_assignment") -> "platform"

            checkId.startsWith("engine.") || checkId.startsWith("frontend.") -> "frontend"

            else -> "runtime"
        }

    fun attach(
        finding: Map<String, Any?>,
        snapshot: Map<String, Any?>,
        now: Long,
        reachable: Boolean,
    ): Map<String, Any?> {
        val id = finding["check_id"] as? String ?: return finding
        val source = source(id)
        val summary = finding["summary_key"] as? String
        val status = finding["status"]
        val quality = snapshot["network_quality"] as? Map<*, *>
        val state =
            when {
                summary == "nq_finding_stale" -> "stale"

                status in setOf("pending", "running", "cancelled") -> "unavailable"

                summary in
                    setOf(
                        "diagnostic_requires_deep_mode",
                        "nq_finding_dns_system",
                        "diagnostic_secure_storage_not_supported",
                    ) -> "not_applicable"

                status == "skipped" -> "unavailable"

                id in
                    setOf(
                        "tunnel.ipv4_egress",
                        "tunnel.ipv6_egress",
                        "protection.dns_path",
                        "protection.route_ownership",
                    ) -> "unavailable"

                !reachable && source in setOf("runtime", "platform", "config") -> "unavailable"

                id in
                    setOf(
                        "physical.ipv4_route",
                        "physical.ipv6_route",
                        "physical.dns_available",
                        "tunnel.routes",
                        "tunnel.dns",
                    ) -> "inferred"

                summary in
                    setOf(
                        "nq_finding_unavailable",
                        "diagnostic_check_failed_internally",
                        "diagnostic_check_timed_out",
                        "diagnostic_event_stream_unknown",
                    ) -> "unavailable"

                source == "runtime" && id !in NetworkDiagnosticChecks.standardIds -> "inferred"

                else -> "observed"
            }
        val sampled =
            if (source == "active_probe") {
                now
            } else if (id in NetworkDiagnosticChecks.standardIds &&
                source == "runtime"
            ) {
                unsigned(quality?.get("sampled_at_unix_ms"))
            } else {
                unsigned(snapshot["observed_at_unix_milliseconds"])
            }
        val observedId = connectionId(snapshot)
        val observed =
            mapOf(
                "source" to source,
                "availability" to state,
                "age_milliseconds" to sampled?.let { (now - it).coerceAtLeast(0) },
                "connection_instance_id" to observedId,
                "network_generation" to if (reachable) unsigned(snapshot["network_generation"]) else null,
            ).filterValues { it != null }
        val legacy = (finding["sanitized_evidence"] as? List<*>).orEmpty()
        val facts = legacy.filterIsInstance<String>().mapNotNull(::fromLegacy).take(16)
        return finding + mapOf("observation" to observed, "evidence" to facts)
    }
}
