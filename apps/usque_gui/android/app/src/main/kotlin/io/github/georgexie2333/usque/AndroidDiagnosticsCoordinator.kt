package io.github.georgexie2333.usque

import java.util.UUID
import java.util.concurrent.Executor

/**
 * Platform-side diagnostic session for Android. It consumes only the
 * read-only, already-sanitized VPN-process snapshot and never opens sockets,
 * creates a TUN, or changes platform network state.
 */
internal class AndroidDiagnosticsCoordinator(
    private val executor: Executor,
    private val publish: (Map<String, Any?>) -> Unit = {},
    private val nowMillis: () -> Long = System::currentTimeMillis,
    private val newSessionId: () -> String = { UUID.randomUUID().toString() },
) {
    class DiagnosticsException(
        val code: String,
        override val message: String,
    ) : IllegalStateException(message)

    private val lock = Any()
    private var generation = 0L
    private var session: Map<String, Any?>? = null
    private var latestSnapshot: Map<String, Any?> = emptyMap()
    private val timeline = ArrayDeque<Map<String, Any?>>()
    private var nextSequence = 1L
    private var attemptStartedAtMillis: Long? = null
    private var lastTimelineFingerprint: String? = null
    private var lastObservedPhase: String? = null
    private var lastNetworkGeneration: Long? = null
    private var observedNetworkChanges = 0L
    private var droppedTimelineEvents = 0L

    fun observeSnapshot(snapshot: Map<String, Any?>) {
        synchronized(lock) {
            latestSnapshot = snapshot.toMap()
            val phase = snapshot["phase"] as? String ?: "disconnected"
            val networkGeneration = (snapshot["network_generation"] as? Number)?.toLong() ?: 0L
            val fingerprint = "$phase:$networkGeneration:${snapshot["error_code"] ?: ""}"
            if (fingerprint == lastTimelineFingerprint) return
            lastTimelineFingerprint = fingerprint
            val previousPhase = lastObservedPhase
            lastObservedPhase = phase
            val previousGeneration = lastNetworkGeneration
            if (previousGeneration != null && previousGeneration != networkGeneration) {
                observedNetworkChanges += 1
            }
            lastNetworkGeneration = networkGeneration
            val now = nowMillis()
            if (
                attemptStartedAtMillis == null ||
                (phase in CONNECTION_START_PHASES && previousPhase !in CONNECTION_START_PHASES)
            ) {
                attemptStartedAtMillis = now
            }
            val failure = failureFromSnapshot(snapshot)
            appendTimeline(
                mapOf(
                    "sequence" to nextSequence++,
                    "timestamp_unix_milliseconds" to now,
                    "elapsed_from_attempt_start_milliseconds" to
                        (now - (attemptStartedAtMillis ?: now)).coerceAtLeast(0L),
                    "event_type" to phaseEvent(phase, previousPhase),
                    "stage" to (failure?.get("stage") ?: phaseStage(phase)),
                    "transport" to safeTransport(snapshot["transport"]),
                    "address_family" to safeFamily(snapshot["address_family"]),
                    "failure" to failure,
                ).filterValues { value -> value != null },
            )
        }
    }

    fun start(
        mode: String,
        snapshot: Map<String, Any?>,
        controlReachable: Boolean,
        eventStreamReachable: Boolean,
        nativeLinked: Boolean,
        nativeReady: Boolean,
    ): Map<String, Any?> {
        val normalizedMode = mode.lowercase()
        if (normalizedMode !in setOf("standard", "deep")) {
            throw DiagnosticsException("INVALID_ARGUMENT", "The diagnostic mode is invalid.")
        }
        val runGeneration: Long
        val sessionId: String
        synchronized(lock) {
            if (session?.get("state") in ACTIVE_STATES) {
                throw DiagnosticsException(
                    "DIAGNOSTICS_ALREADY_RUNNING",
                    "Another diagnostic session is already active.",
                )
            }
            latestSnapshot = snapshot.toMap()
            generation += 1
            runGeneration = generation
            sessionId = newSessionId()
            session =
                mapOf(
                    "session_id" to sessionId,
                    "state" to "running",
                    "started_at_unix_milliseconds" to nowMillis(),
                    "mode" to normalizedMode,
                    "current_check" to CHECKS.first().id,
                    "progress_percent" to 0,
                    "findings" to CHECKS.map(::pendingFinding),
                    "summary" to emptySummary(),
                )
        }
        publishSession(requireNotNull(current()))
        executor.execute {
            complete(
                runGeneration = runGeneration,
                sessionId = sessionId,
                mode = normalizedMode,
                snapshot = snapshot.toMap(),
                controlReachable = controlReachable,
                eventStreamReachable = eventStreamReachable,
                nativeLinked = nativeLinked,
                nativeReady = nativeReady,
            )
        }
        return requireNotNull(current())
    }

    fun cancel(sessionId: String): Map<String, Any?> {
        val cancelled: Map<String, Any?>
        synchronized(lock) {
            val current =
                session
                    ?: throw DiagnosticsException("DIAGNOSTICS_NOT_FOUND", "No diagnostic session exists.")
            if (current["session_id"] != sessionId) {
                throw DiagnosticsException(
                    "DIAGNOSTICS_SESSION_MISMATCH",
                    "The diagnostic session identifier does not match.",
                )
            }
            if (current["state"] !in ACTIVE_STATES) return current.toMap()
            generation += 1
            val findings =
                (current["findings"] as? List<*>)
                    ?.mapNotNull(::stringMap)
                    ?.map { finding ->
                        if (finding["status"] in setOf("pending", "running")) {
                            finding + ("status" to "cancelled")
                        } else {
                            finding
                        }
                    }.orEmpty()
            cancelled =
                current +
                mapOf(
                    "state" to "cancelled",
                    "completed_at_unix_milliseconds" to nowMillis(),
                    "current_check" to null,
                    "progress_percent" to 100,
                    "findings" to findings,
                    "summary" to summarize(findings),
                )
            session = cancelled
        }
        publishSession(cancelled)
        return cancelled.toMap()
    }

    fun current(): Map<String, Any?>? = synchronized(lock) { session?.toMap() }

    fun matchesSession(sessionId: String?): Boolean =
        sessionId == null || synchronized(lock) { session?.get("session_id") == sessionId }

    fun timeline(): Map<String, Any?> =
        synchronized(lock) {
            val snapshot = latestSnapshot
            mapOf(
                "events" to timeline.map(Map<String, Any?>::toMap),
                "metrics" to
                    mapOf(
                        "reconnect_count" to
                            ((snapshot["reconnect_count"] as? Number)?.toLong() ?: 0L),
                        "fallback_count" to 0L,
                        "network_change_count" to observedNetworkChanges,
                        "send_queue_high_watermark" to 0L,
                        "send_queue_drop_count" to 0L,
                        "current_smoothed_rtt_known" to false,
                        "last_failure_code" to (snapshot["error_code"] as? String),
                    ).filterValues { value -> value != null },
                "dropped_event_count" to droppedTimelineEvents,
            )
        }

    private fun complete(
        runGeneration: Long,
        sessionId: String,
        mode: String,
        snapshot: Map<String, Any?>,
        controlReachable: Boolean,
        eventStreamReachable: Boolean,
        nativeLinked: Boolean,
        nativeReady: Boolean,
    ) {
        val findings = CHECKS.map(::pendingFinding).toMutableList()
        for ((index, check) in CHECKS.withIndex()) {
            val running =
                synchronized(lock) {
                    if (generation != runGeneration || session?.get("session_id") != sessionId) {
                        return
                    }
                    findings[index] =
                        findings[index] +
                        mapOf(
                            "status" to "running",
                            "started_at_unix_milliseconds" to nowMillis(),
                        )
                    val next =
                        requireNotNull(session) +
                            mapOf(
                                "current_check" to check.id,
                                "findings" to findings.toList(),
                                "progress_percent" to ((index * 100) / CHECKS.size),
                                "summary" to summarize(findings),
                            )
                    session = next
                    next
                }
            publishSession(running)

            val finding =
                evaluate(
                    check = check,
                    mode = mode,
                    snapshot = snapshot,
                    controlReachable = controlReachable,
                    eventStreamReachable = eventStreamReachable,
                    nativeLinked = nativeLinked,
                    nativeReady = nativeReady,
                )
            val updated =
                synchronized(lock) {
                    if (generation != runGeneration || session?.get("session_id") != sessionId) {
                        return
                    }
                    findings[index] = finding
                    val next =
                        requireNotNull(session) +
                            mapOf(
                                "current_check" to null,
                                "findings" to findings.toList(),
                                "progress_percent" to (((index + 1) * 100) / CHECKS.size),
                                "summary" to summarize(findings),
                            )
                    session = next
                    next
                }
            publishSession(updated)
        }
        val completed: Map<String, Any?>
        synchronized(lock) {
            if (generation != runGeneration || session?.get("session_id") != sessionId) return
            completed =
                requireNotNull(session) +
                mapOf(
                    "state" to "completed",
                    "completed_at_unix_milliseconds" to nowMillis(),
                    "current_check" to null,
                    "progress_percent" to 100,
                    "findings" to findings,
                    "summary" to summarize(findings),
                )
            session = completed
        }
        publishSession(completed)
    }

    private fun evaluate(
        check: Check,
        mode: String,
        snapshot: Map<String, Any?>,
        controlReachable: Boolean,
        eventStreamReachable: Boolean,
        nativeLinked: Boolean,
        nativeReady: Boolean,
    ): Map<String, Any?> {
        val phase = snapshot["phase"] as? String ?: "disconnected"
        val connected = phase == "connected" || phase == "degraded"
        val familyMask = (snapshot["underlying_family_mask"] as? Number)?.toInt() ?: 0
        val hasNetwork = snapshot["underlying_network_present"] == true
        val tunOpen = snapshot["tun_fd_valid"] == true
        val dnsCount = (snapshot["dns_server_count"] as? Number)?.toInt() ?: 0
        val activeFrontends = snapshot["active_frontends"] as? List<*> ?: emptyList<Any?>()
        val tunnelIpv4Available = snapshot["tunnel_ipv4_available"] == true
        val tunnelIpv6Available = snapshot["tunnel_ipv6_available"] == true
        val platformStateObserved = controlReachable && snapshot["platform_state_observed"] == true
        val errorCode = (snapshot["error_code"] as? String)?.takeIf(SAFE_CODE::matches)
        val status: String
        val evidence = mutableListOf<String>()
        var failure: Map<String, Any?>? = null

        when (check.id) {
            "engine.control_channel" -> {
                status = if (controlReachable) "passed" else "failed"
            }

            "engine.event_stream" -> {
                status = if (eventStreamReachable) "passed" else "warning"
            }

            "engine.capabilities" -> {
                status = if (nativeLinked) "passed" else "failed"
            }

            "engine.configuration" -> {
                status =
                    if (!controlReachable) {
                        "skipped"
                    } else if (errorCode == "CONFIGURATION_INVALID") {
                        "failed"
                    } else {
                        "passed"
                    }
            }

            "engine.secure_storage_metadata" -> {
                status = "warning"
            }

            "frontend.socks_port" -> {
                status =
                    if (controlReachable && "socks5" in activeFrontends) "passed" else "skipped"
            }

            "frontend.http_port" -> {
                status =
                    if (controlReachable && "http" in activeFrontends) "passed" else "skipped"
            }

            "frontend.system_proxy_state" -> {
                status = "skipped"
            }

            "physical.network_present" -> {
                status =
                    if (!platformStateObserved) {
                        "skipped"
                    } else if (hasNetwork) {
                        "passed"
                    } else {
                        "warning"
                    }
                evidence +=
                    "network=${
                        if (!platformStateObserved) {
                            "unknown"
                        } else if (hasNetwork) {
                            "present"
                        } else {
                            "absent"
                        }
                    }"
            }

            "physical.ipv4_route" -> {
                status =
                    if (!platformStateObserved) {
                        "skipped"
                    } else if (familyMask and 1 != 0) {
                        "passed"
                    } else {
                        "warning"
                    }
            }

            "physical.ipv6_route" -> {
                status =
                    if (!platformStateObserved) {
                        "skipped"
                    } else if (familyMask and 2 != 0) {
                        "passed"
                    } else {
                        "warning"
                    }
            }

            "physical.dns_available" -> {
                status =
                    if (!platformStateObserved) {
                        "skipped"
                    } else if (dnsCount > 0) {
                        "passed"
                    } else {
                        "warning"
                    }
                evidence +=
                    if (platformStateObserved) "dns_server_count=$dnsCount" else "dns_server_count=unknown"
            }

            "physical.network_generation" -> {
                val generation = snapshot["network_generation"] as? Number
                status =
                    if (!platformStateObserved) {
                        "skipped"
                    } else if (generation != null) {
                        "passed"
                    } else {
                        "warning"
                    }
                generation?.let { evidence += "generation=${it.toLong()}" }
            }

            "transport.h3_connect", "transport.h3_datagram" -> {
                status =
                    if (connected &&
                        controlReachable &&
                        snapshot["transport"] == "h3"
                    ) {
                        "passed"
                    } else {
                        "skipped"
                    }
            }

            "transport.h2_tcp", "transport.h2_tls", "transport.h2_connect" -> {
                status =
                    if (connected &&
                        controlReachable &&
                        snapshot["transport"] == "h2"
                    ) {
                        "passed"
                    } else {
                        "skipped"
                    }
            }

            "transport.endpoint_pin", "transport.fallback_policy" -> {
                status =
                    if (!controlReachable) {
                        "skipped"
                    } else if (errorCode == "ENDPOINT_PIN_MISMATCH") {
                        "failed"
                    } else if (nativeReady) {
                        "passed"
                    } else {
                        "skipped"
                    }
            }

            "tunnel.address_assignment" -> {
                status =
                    if (!platformStateObserved) {
                        "skipped"
                    } else if (
                        connected &&
                        tunOpen &&
                        (tunnelIpv4Available || tunnelIpv6Available)
                    ) {
                        "passed"
                    } else if (connected) {
                        "failed"
                    } else {
                        "skipped"
                    }
            }

            "tunnel.routes", "tunnel.dns" -> {
                status =
                    if (!platformStateObserved) {
                        "skipped"
                    } else if (connected && tunOpen) {
                        // The service confirms its configured state, but only an external
                        // observer can prove effective OS routing or DNS ownership.
                        "warning"
                    } else if (connected) {
                        "failed"
                    } else {
                        "skipped"
                    }
            }

            "tunnel.first_packet" -> {
                val bytes =
                    ((snapshot["downloaded_bytes"] as? Number)?.toLong() ?: 0L) +
                        ((snapshot["uploaded_bytes"] as? Number)?.toLong() ?: 0L)
                status =
                    if (!controlReachable) {
                        "skipped"
                    } else if (bytes > 0L) {
                        "passed"
                    } else if (connected) {
                        "warning"
                    } else {
                        "skipped"
                    }
            }

            "tunnel.ipv4_egress", "tunnel.ipv6_egress" -> {
                status = if (mode == "deep") "warning" else "skipped"
            }

            "protection.kill_switch" -> {
                val killSwitchState = snapshot["kill_switch_state"] as? String
                status =
                    if (!platformStateObserved) {
                        "skipped"
                    } else {
                        when (killSwitchState) {
                            "active" -> "passed"
                            "notApplicable" -> "skipped"
                            else -> "warning"
                        }
                    }
                evidence += "kill_switch=${killSwitchState ?: "unknown"}"
            }

            "protection.dns_path", "protection.route_ownership" -> {
                status =
                    if (connected && tunOpen) "warning" else "skipped"
            }

            "protection.recovery_journal" -> {
                status =
                    if (!platformStateObserved) {
                        "skipped"
                    } else if (snapshot["pending_cleanup"] == true) {
                        "failed"
                    } else {
                        "passed"
                    }
            }

            else -> {
                status = "skipped"
            }
        }

        if (status == "failed") {
            val code = errorCode ?: defaultFailureCode(check.id)
            failure = failureFromSnapshot(snapshot) ?: fallbackFailure(code)
        }
        return mapOf(
            "check_id" to check.id,
            "category" to check.category,
            "status" to status,
            "failure" to failure,
            "severity" to
                if (status == "failed") {
                    "error"
                } else if (status == "warning") {
                    "warning"
                } else {
                    "info"
                },
            "summary_key" to "diagnostics.${check.id}.$status",
            "remediation_key" to
                if (check.id.endsWith("egress") || check.id in INDEPENDENT_OBSERVER_CHECKS) {
                    "run_release_leak_gate"
                } else if (status == "warning" || status == "failed") {
                    "inspect_platform_state"
                } else {
                    "none"
                },
            "sanitized_evidence" to evidence,
            "started_at_unix_milliseconds" to nowMillis(),
            "duration_milliseconds" to 0L,
        ).filterValues { value -> value != null }
    }

    private fun publishSession(value: Map<String, Any?>) {
        publish(mapOf("diagnostic_session" to value))
    }

    private fun appendTimeline(event: Map<String, Any?>) {
        if (timeline.size == MAX_TIMELINE_EVENTS) {
            timeline.removeFirst()
            droppedTimelineEvents += 1
        }
        timeline.addLast(event)
    }

    private fun stringMap(value: Any?): Map<String, Any?>? {
        val raw = value as? Map<*, *> ?: return null
        if (raw.keys.any { key -> key !is String }) return null
        return raw.entries.associate { (key, entryValue) -> (key as String) to entryValue }
    }

    private fun pendingFinding(check: Check): Map<String, Any?> =
        mapOf(
            "check_id" to check.id,
            "category" to check.category,
            "status" to "pending",
            "severity" to "info",
            "summary_key" to "diagnostics.${check.id}.pending",
            "remediation_key" to "none",
            "sanitized_evidence" to emptyList<String>(),
        )

    private fun summarize(findings: List<Map<String, Any?>>): Map<String, Long> =
        mapOf(
            "passed" to findings.count { it["status"] == "passed" }.toLong(),
            "warnings" to findings.count { it["status"] == "warning" }.toLong(),
            "failed" to findings.count { it["status"] == "failed" }.toLong(),
            "skipped" to findings.count { it["status"] == "skipped" }.toLong(),
            "cancelled" to findings.count { it["status"] == "cancelled" }.toLong(),
        )

    private fun emptySummary(): Map<String, Long> =
        mapOf("passed" to 0L, "warnings" to 0L, "failed" to 0L, "skipped" to 0L, "cancelled" to 0L)

    private fun failureFromSnapshot(snapshot: Map<String, Any?>): Map<String, Any?>? {
        val code = (snapshot["error_code"] as? String)?.takeIf(SAFE_CODE::matches) ?: return null
        val structured = stringMap(snapshot["failure"])
        if (structured != null && structured["code"] == code) {
            val stage = (structured["stage"] as? String)?.takeIf(SAFE_TOKEN::matches)
            if (stage != null) {
                return mapOf(
                    "code" to code,
                    "stage" to stage,
                    "transport" to
                        (structured["transport"] as? String)?.takeIf(SAFE_TOKEN::matches),
                    "address_family" to
                        (structured["address_family"] as? String)?.takeIf(SAFE_TOKEN::matches),
                    "retryable" to (structured["retryable"] as? Boolean ?: false),
                    "fallback_allowed" to
                        (structured["fallback_allowed"] as? Boolean ?: false),
                    "severity" to
                        (structured["severity"] as? String)?.takeIf(SAFE_TOKEN::matches),
                    "remediation_key" to
                        (structured["remediation_key"] as? String)?.takeIf(SAFE_TOKEN::matches),
                    "sanitized_detail" to
                        (structured["sanitized_detail"] as? String)?.takeIf(::safeFailureDetail),
                ).filterValues { value -> value != null }
            }
        }
        return fallbackFailure(code)
    }

    private fun fallbackFailure(code: String): Map<String, Any?> =
        mapOf(
            "code" to code,
            "stage" to failureStage(code),
            "retryable" to true,
            "fallback_allowed" to (code in H3_FALLBACK_CODES),
            "severity" to "error",
            "remediation_key" to
                if (code in H3_FALLBACK_CODES) "try_http2" else "inspect_platform_state",
        )

    private fun defaultFailureCode(checkId: String): String =
        when (checkId) {
            "engine.control_channel" -> "ENGINE_UNAVAILABLE"
            "engine.capabilities" -> "VPN_SERVICE_UNAVAILABLE"
            "engine.configuration" -> "CONFIGURATION_INVALID"
            "transport.endpoint_pin" -> "ENDPOINT_PIN_MISMATCH"
            "tunnel.address_assignment" -> "TUN_ADDRESS_MISSING"
            "tunnel.routes" -> "ROUTE_APPLY_FAILED"
            "tunnel.dns" -> "DNS_APPLY_FAILED"
            "protection.recovery_journal" -> "PLATFORM_RECOVERY_PENDING"
            else -> "INTERNAL"
        }

    private fun safeFailureDetail(value: String): Boolean =
        value.length <= 64 &&
            FAILURE_DETAIL_PREFIXES.any { prefix ->
                value.removePrefix(prefix).let { suffix ->
                    suffix.length < value.length &&
                        suffix.isNotEmpty() &&
                        suffix.all { character -> character in '0'..'9' }
                }
            }

    private fun safeTransport(value: Any?): String? =
        (value as? String)?.lowercase()?.takeIf { it == "h2" || it == "h3" }

    private fun safeFamily(value: Any?): String? =
        (value as? String)?.lowercase()?.takeIf { it == "ipv4" || it == "ipv6" || it == "dual" }

    private fun phaseEvent(
        phase: String,
        previousPhase: String?,
    ): String =
        when (phase) {
            "connectingH2" -> {
                if (previousPhase == "connectingH3") "fallback_started" else "attempt_started"
            }

            "preparing", "connectingH3" -> {
                "attempt_started"
            }

            "connected", "degraded" -> {
                "tunnel_ready"
            }

            "reconnecting" -> {
                "reconnect_scheduled"
            }

            "error" -> {
                "failed"
            }

            else -> {
                "disconnected"
            }
        }

    private fun phaseStage(phase: String): String =
        when (phase) {
            "connectingH3" -> "quic_handshake"
            "connectingH2" -> "tls_handshake"
            "connected", "degraded" -> "tunnel_startup"
            "error" -> "platform_recovery"
            else -> "tunnel_startup"
        }

    private fun failureStage(code: String): String =
        when {
            code.startsWith("H3_") -> "quic_handshake"
            code == "H2_TLS_FAILED" -> "tls_handshake"
            code.startsWith("H2_") -> "masque_connect"
            code == "ENDPOINT_PIN_MISMATCH" -> "tls_handshake"
            code.startsWith("PHYSICAL_") -> "endpoint_resolution"
            code.startsWith("DNS_") -> "dns_apply"
            code.startsWith("ROUTE_") -> "route_apply"
            code.startsWith("KILL_SWITCH_") -> "kill_switch_apply"
            code.startsWith("PACKET_SEND") || code == "SEND_QUEUE_FULL" -> "packet_send"
            code.startsWith("PACKET_RECEIVE") -> "packet_receive"
            else -> "platform_recovery"
        }

    private data class Check(
        val id: String,
        val category: String,
    )

    companion object {
        private const val MAX_TIMELINE_EVENTS = 256
        private val ACTIVE_STATES = setOf("pending", "running", "cancelling")
        private val CONNECTION_START_PHASES = setOf("preparing", "connectingH3", "connectingH2")
        private val SAFE_CODE = Regex("^[A-Z][A-Z0-9_]{1,63}$")
        private val SAFE_TOKEN = Regex("^[a-z][a-z0-9_]{0,63}$")
        private val FAILURE_DETAIL_PREFIXES =
            setOf("attempt ", "status ", "generation ", "queue depth ")
        private val H3_FALLBACK_CODES =
            setOf(
                "H3_UDP_UNREACHABLE",
                "H3_HANDSHAKE_TIMEOUT",
                "H3_PROTOCOL_ERROR",
                "H3_DATAGRAM_UNAVAILABLE",
                "H3_CONNECTION_CLOSED",
            )
        private val INDEPENDENT_OBSERVER_CHECKS =
            setOf("protection.dns_path", "protection.route_ownership")
        private val CHECKS =
            listOf(
                Check("engine.control_channel", "local_component"),
                Check("engine.event_stream", "local_component"),
                Check("engine.capabilities", "local_component"),
                Check("engine.configuration", "local_component"),
                Check("engine.secure_storage_metadata", "local_component"),
                Check("frontend.socks_port", "local_component"),
                Check("frontend.http_port", "local_component"),
                Check("frontend.system_proxy_state", "local_component"),
                Check("physical.network_present", "physical_network"),
                Check("physical.ipv4_route", "physical_network"),
                Check("physical.ipv6_route", "physical_network"),
                Check("physical.dns_available", "physical_network"),
                Check("physical.network_generation", "physical_network"),
                Check("transport.h3_connect", "transport"),
                Check("transport.h3_datagram", "transport"),
                Check("transport.h2_tcp", "transport"),
                Check("transport.h2_tls", "transport"),
                Check("transport.h2_connect", "transport"),
                Check("transport.endpoint_pin", "transport"),
                Check("transport.fallback_policy", "transport"),
                Check("tunnel.address_assignment", "tunnel"),
                Check("tunnel.routes", "tunnel"),
                Check("tunnel.dns", "tunnel"),
                Check("tunnel.first_packet", "tunnel"),
                Check("tunnel.ipv4_egress", "tunnel"),
                Check("tunnel.ipv6_egress", "tunnel"),
                Check("protection.kill_switch", "protection"),
                Check("protection.dns_path", "protection"),
                Check("protection.route_ownership", "protection"),
                Check("protection.recovery_journal", "recovery"),
            )
    }
}
