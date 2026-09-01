package io.github.georgexie2333.usque

import android.annotation.SuppressLint
import android.content.Context
import android.net.Uri
import android.os.Build
import androidx.core.content.edit
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException
import java.security.MessageDigest
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream

internal object AndroidMaintenance {
    private const val UPDATE_PREFERENCES = "usque_update_state_v1"
    private const val MAX_UPDATE_RESULT_BYTES = 16 * 1024
    private const val MAX_DIAGNOSTIC_LOG_BYTES = 2 * 1024 * 1024
    private const val MAX_DIAGNOSTIC_BUNDLE_BYTES = 8 * 1024 * 1024
    private const val RELEASE_URL_PREFIX =
        "https://github.com/GeorgeXie2333/usque-app/releases/"
    private const val RELEASE_DOWNLOAD_PREFIX =
        "https://github.com/GeorgeXie2333/usque-app/releases/download/"
    private const val MAX_UPDATE_PACKAGE_BYTES = 512L * 1024L * 1024L
    private val UPDATE_SHA256 = Regex("^[0-9a-f]{64}$")
    private val UPDATE_VARIANTS = setOf("arm64-v8a", "x86_64", "armeabi-v7a")

    @Suppress("UNUSED_PARAMETER")
    fun checkForUpdates(
        context: Context,
        manual: Boolean,
    ): Map<String, Any?> {
        // `manual` remains part of the MethodChannel contract, but every call
        // is live now. Clear the obsolete 24-hour cache left by older builds.
        cleanupLegacyUpdateState(context)
        val response =
            NativeEngine.checkForUpdates()
                ?: throw IOException("The Rust update checker is unavailable.")
        if (response.toByteArray(Charsets.UTF_8).size > MAX_UPDATE_RESULT_BYTES) {
            throw IOException("The update result exceeded the Android safety limit.")
        }
        return parseUpdateResult(response)
    }

    fun cleanupLegacyUpdateState(context: Context) {
        context
            .getSharedPreferences(UPDATE_PREFERENCES, Context.MODE_PRIVATE)
            .edit { clear() }
    }

    fun writeDiagnostics(
        context: Context,
        destination: Uri,
        snapshot: Map<String, Any?>,
        diagnosticSession: Map<String, Any?>? = null,
        connectionTimeline: Map<String, Any?> = emptyMap(),
    ) {
        val logs = AndroidLogStore(context).diagnosticSnapshot(MAX_DIAGNOSTIC_LOG_BYTES)
        val connection =
            JSONObject()
                .put("phase", safeEnum(snapshot["phase"], CONNECTION_PHASES, "unknown"))
                .put("transport", safeEnum(snapshot["transport"], setOf("h2", "h3"), null))
                .put(
                    "address_family",
                    safeEnum(snapshot["address_family"], setOf("ipv4", "ipv6", "dual"), null),
                ).put("reconnect_count", safeCounter(snapshot["reconnect_count"]))
                .put(
                    "kill_switch_state",
                    safeEnum(snapshot["kill_switch_state"], KILL_SWITCH_STATES, "unknown"),
                ).put("platform_lockdown", snapshot["platform_lockdown"] == true)
                .put("always_on", snapshot["always_on"] == true)
                .put(
                    "active_listener_count",
                    (snapshot["active_listeners"] as? List<*>)?.size?.coerceAtMost(32) ?: 0,
                ).put("exit_ipv4_observed", snapshot["exit_ipv4"] != null)
                .put("exit_ipv6_observed", snapshot["exit_ipv6"] != null)
        val configuration =
            JSONObject()
                .put("platform", "android")
                .put("vpn_service_diagnostics", true)
                .put("diagnostic_modes", listOf("standard", "deep"))
                .put("automatic_upload", false)
        val platformHealth =
            JSONObject()
                .put(
                    "vpn_service_state",
                    safeEnum(snapshot["vpn_service_state"], SERVICE_STATES, "unknown"),
                ).put(
                    "vpn_process_state",
                    safeEnum(snapshot["vpn_process_state"], PROCESS_STATES, "unknown"),
                ).put("tun_fd_valid", snapshot["tun_fd_valid"] == true)
                .put("tun_interface_present", snapshot["tun_interface_present"] == true)
                .put(
                    "underlying_network_present",
                    snapshot["underlying_network_present"] == true,
                ).put("underlying_family_mask", safeCounter(snapshot["underlying_family_mask"]))
                .put("network_generation", safeCounter(snapshot["network_generation"]))
                .put("dns_server_count", safeCounter(snapshot["dns_server_count"]))
                .put("always_on_state", snapshot["always_on"] == true)
                .put("lockdown_state", snapshot["platform_lockdown"] == true)
                .put(
                    "foreground_notification_state",
                    safeEnum(
                        snapshot["foreground_notification_state"],
                        NOTIFICATION_STATES,
                        "unknown",
                    ),
                ).put(
                    "native_runtime_state",
                    safeEnum(snapshot["native_runtime_state"], RUNTIME_STATES, "unknown"),
                ).put("pending_cleanup", snapshot["pending_cleanup"] == true)
                .put("independent_leak_verification", false)
        val readme =
            """
            Usque diagnostic bundle

            This archive was created locally and is never uploaded automatically.
            Identity secrets, cryptographic material, full network addresses, SSIDs,
            installed-app lists, and user-provided profile names are deliberately excluded.
            Leak safety is established only by the independent release test environment.
            """.trimIndent() + "\n"

        val payloads = linkedMapOf<String, ByteArray>()
        payloads["configuration-summary.json"] = configuration.toString(2).toByteArray()
        payloads["connection-summary.json"] = connection.toString(2).toByteArray()
        payloads["connection-timeline.json"] =
            sanitizeConnectionTimeline(connectionTimeline).toString(2).toByteArray()
        payloads["platform-health.json"] = platformHealth.toString(2).toByteArray()
        val sanitizedSession = diagnosticSession?.let(::sanitizeDiagnosticSession)
        if (diagnosticSession != null) {
            payloads["diagnostic-session.json"] =
                requireNotNull(sanitizedSession).toString(2).toByteArray()
        }
        if (logs.isNotEmpty()) payloads["logs/android-engine.jsonl"] = logs.toByteArray()
        payloads["README.txt"] = readme.toByteArray()
        val payloadBytes = payloads.values.sumOf(ByteArray::size)
        if (payloadBytes > MAX_DIAGNOSTIC_BUNDLE_BYTES) {
            throw IOException("The diagnostic bundle exceeded the local safety limit.")
        }

        val sessionState = sanitizedSession?.optString("state")
        val manifest =
            JSONObject()
                .put("schema_version", 2)
                .put("created_at_unix_millis", System.currentTimeMillis())
                .put(
                    "app_version",
                    context.packageManager.getPackageInfo(context.packageName, 0).versionName,
                ).put("platform", "android")
                .put("sdk", Build.VERSION.SDK_INT)
                .put("supported_abis", Build.SUPPORTED_ABIS.joinToString(","))
                .put("diagnostic_complete", sessionState == "completed")
                .put("diagnostic_cancelled", sessionState == "cancelled")
                .put("redaction_policy", "allowlist-v2")
                .put(
                    "contents",
                    payloads.map { (name, bytes) ->
                        mapOf("path" to name, "size" to bytes.size, "sha256" to sha256(bytes))
                    },
                ).put(
                    "excluded",
                    listOf(
                        "WARP Secret",
                        "private key",
                        "access token",
                        "device ID",
                        "license",
                        "endpoint pin",
                        "full IP addresses and hostnames",
                        "listener addresses",
                        "custom endpoint and DNS addresses",
                        "split-exclusion CIDRs",
                        "SSID",
                        "installed application list",
                        "user filesystem paths",
                    ),
                )
        val output =
            context.contentResolver.openOutputStream(destination, "rwt")
                ?: throw IOException("The selected document provider returned no output stream.")
        output.use { stream ->
            ZipOutputStream(stream.buffered()).use { archive ->
                archive.writeEntry("manifest.json", manifest.toString(2).toByteArray())
                payloads.forEach { (name, bytes) -> archive.writeEntry(name, bytes) }
            }
        }
    }

    // Clear-all must know whether persistence succeeded before continuing.
    @SuppressLint("ApplySharedPref", "UseKtx")
    fun clearLocalState(context: Context) {
        check(
            context
                .getSharedPreferences(UPDATE_PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .clear()
                .commit(),
        ) {
            "Android update state could not be cleared"
        }
        AndroidLogStore(context).clear()
        FlagSvgCache(context).clear()
        PerAppProxyStore.clear(context)
    }

    internal fun parseUpdateResult(json: String): Map<String, Any?> {
        val value = JSONObject(json)
        val available = value.optBoolean("available", false)
        val version = value.optString("version").takeIf { it.length in 1..64 }
        val releaseUrl =
            value
                .optString("release_url")
                .takeIf { it.length <= 512 && it.startsWith(RELEASE_URL_PREFIX) }
        if (available && (version == null || releaseUrl == null)) {
            throw IOException("The Rust update checker returned an invalid release.")
        }
        val updatePackage =
            value.optJSONObject("package")?.let { packageValue ->
                val name = packageValue.optString("name")
                val downloadUrl = packageValue.optString("download_url")
                val size = packageValue.optLong("size", 0L)
                val sha256 = packageValue.optString("sha256").lowercase()
                val platform = packageValue.optString("platform")
                val variant = packageValue.optString("variant")
                val expectedUrl = "$RELEASE_DOWNLOAD_PREFIX$version/$name"
                if (
                    name.isEmpty() ||
                    name.length > 160 ||
                    downloadUrl != expectedUrl ||
                    size !in 1..MAX_UPDATE_PACKAGE_BYTES ||
                    !UPDATE_SHA256.matches(sha256) ||
                    platform != "android" ||
                    variant !in UPDATE_VARIANTS ||
                    name != "usque-$version-android-$variant.apk"
                ) {
                    throw IOException("The Rust update checker returned an invalid Android package.")
                }
                mapOf(
                    "name" to name,
                    "download_url" to downloadUrl,
                    "size" to size,
                    "sha256" to sha256,
                    "platform" to platform,
                    "variant" to variant,
                )
            }
        return mapOf(
            "available" to available,
            "version" to version,
            "release_url" to releaseUrl,
            "package" to updatePackage,
        )
    }

    internal fun sanitizeDiagnosticSession(source: Map<String, Any?>): JSONObject {
        val startedAt = safeCounter(source["started_at_unix_milliseconds"])
        val state = safeEnum(source["state"], SESSION_STATES, "failed") ?: "failed"
        val mode = safeEnum(source["mode"], DIAGNOSTIC_MODES, "standard") ?: "standard"
        val findings = JSONArray()
        val statuses = mutableListOf<String>()
        val rawFindings = source["findings"] as? List<*> ?: emptyList<Any?>()
        for (rawFinding in rawFindings.take(MAX_DIAGNOSTIC_FINDINGS)) {
            val finding = stringMap(rawFinding) ?: continue
            val checkId = (finding["check_id"] as? String)?.takeIf(CHECK_IDS::contains) ?: continue
            val status =
                safeEnum(finding["status"], CHECK_STATUSES, "skipped") ?: "skipped"
            statuses += status
            val output =
                JSONObject()
                    .put("check_id", checkId)
                    .put("category", categoryForCheck(checkId))
                    .put("status", status)
                    .put(
                        "severity",
                        safeEnum(finding["severity"], SEVERITIES, "info") ?: "info",
                    ).put("duration_milliseconds", safeCounter(finding["duration_milliseconds"]))
            val expectedSummary = "diagnostics.$checkId.$status"
            if (finding["summary_key"] == expectedSummary) {
                output.put("summary_key", expectedSummary)
            }
            safeRemediationKey(finding["remediation_key"])?.let { key ->
                output.put("remediation_key", key)
            }
            val evidence = JSONArray()
            (finding["sanitized_evidence"] as? List<*>)
                ?.asSequence()
                ?.filterIsInstance<String>()
                ?.filter(::safeEvidence)
                ?.take(MAX_EVIDENCE_ITEMS)
                ?.forEach(evidence::put)
            output.put("sanitized_evidence", evidence)
            val findingStarted = safeCounter(finding["started_at_unix_milliseconds"])
            if (startedAt > 0 && findingStarted >= startedAt) {
                output.put("started_after_milliseconds", findingStarted - startedAt)
            }
            (finding["dependency_reason"] as? String)
                ?.takeIf(CHECK_IDS::contains)
                ?.let { dependency -> output.put("dependency_reason", dependency) }
            sanitizeFailure(finding["failure"])?.let { failure ->
                output.put("failure", failure)
            }
            findings.put(output)
        }
        val output =
            JSONObject()
                .put("schema_version", 1)
                .put(
                    "session_id",
                    (source["session_id"] as? String)?.takeIf(SESSION_ID::matches) ?: "anonymous",
                ).put("state", state)
                .put("mode", mode)
                .put("progress_percent", safeCounter(source["progress_percent"]).coerceAtMost(100))
                .put("findings", findings)
                .put("summary", diagnosticSummary(statuses))
        (source["current_check"] as? String)
            ?.takeIf(CHECK_IDS::contains)
            ?.let { current -> output.put("current_check", current) }
        val completedAt = safeCounter(source["completed_at_unix_milliseconds"])
        if (startedAt > 0 && completedAt >= startedAt) {
            output.put("completed_after_milliseconds", completedAt - startedAt)
        }
        return output
    }

    internal fun sanitizeConnectionTimeline(source: Map<String, Any?>): JSONObject {
        val events = JSONArray()
        val rawEvents = source["events"] as? List<*> ?: emptyList<Any?>()
        for (rawEvent in rawEvents.takeLast(MAX_TIMELINE_EVENTS)) {
            val event = stringMap(rawEvent) ?: continue
            val eventType = safeEnum(event["event_type"], EVENT_TYPES, null) ?: continue
            val output =
                JSONObject()
                    .put("sequence", safeCounter(event["sequence"]))
                    .put(
                        "elapsed_from_attempt_start_milliseconds",
                        safeCounter(event["elapsed_from_attempt_start_milliseconds"]),
                    ).put("event_type", eventType)
            safeEnum(event["stage"], TRANSPORT_STAGES, null)?.let { stage ->
                output.put("stage", stage)
            }
            safeEnum(event["transport"], TRANSPORTS, null)?.let { transport ->
                output.put("transport", transport)
            }
            safeEnum(event["address_family"], ADDRESS_FAMILIES, null)?.let { family ->
                output.put("address_family", family)
            }
            (event["duration_milliseconds"] as? Number)?.let { duration ->
                output.put("duration_milliseconds", safeCounter(duration))
            }
            sanitizeFailure(event["failure"])?.let { failure ->
                output.put("failure", failure)
            }
            events.put(output)
        }
        val metricsSource = stringMap(source["metrics"]).orEmpty()
        val metrics = JSONObject()
        for (key in DURATION_METRICS) {
            (metricsSource[key] as? Number)?.let { value ->
                metrics.put(key, safeCounter(value))
            }
        }
        for (key in COUNTER_METRICS) {
            metrics.put(key, safeCounter(metricsSource[key]))
        }
        if (metricsSource["current_smoothed_rtt_known"] == true) {
            metrics.put(
                "current_smoothed_rtt_milliseconds",
                safeCounter(metricsSource["current_smoothed_rtt_milliseconds"]),
            )
        }
        (metricsSource["last_failure_code"] as? String)
            ?.takeIf(FAILURE_CODES::contains)
            ?.let { code -> metrics.put("last_failure_code", code) }
        (metricsSource["last_reconnect_code"] as? String)
            ?.takeIf(FAILURE_CODES::contains)
            ?.let { code -> metrics.put("last_reconnect_code", code) }
        return JSONObject()
            .put("schema_version", 1)
            .put("events", events)
            .put("metrics", metrics)
            .put("dropped_event_count", safeCounter(source["dropped_event_count"]))
    }

    private fun sanitizeFailure(value: Any?): JSONObject? {
        val source = stringMap(value) ?: return null
        val code = (source["code"] as? String)?.takeIf(FAILURE_CODES::contains) ?: return null
        val stage = safeEnum(source["stage"], TRANSPORT_STAGES, null) ?: return null
        val output =
            JSONObject()
                .put("code", code)
                .put("stage", stage)
                .put("retryable", source["retryable"] == true)
                .put("fallback_allowed", source["fallback_allowed"] == true)
                .put("severity", safeEnum(source["severity"], SEVERITIES, "error"))
        safeEnum(source["transport"], TRANSPORTS, null)?.let { transport ->
            output.put("transport", transport)
        }
        safeEnum(source["address_family"], ADDRESS_FAMILIES, null)?.let { family ->
            output.put("address_family", family)
        }
        safeRemediationKey(source["remediation_key"])?.let { remediation ->
            output.put("remediation_key", remediation)
        }
        (source["sanitized_detail"] as? String)
            ?.takeIf(::safeFailureDetail)
            ?.let { detail -> output.put("sanitized_detail", detail) }
        return output
    }

    private fun diagnosticSummary(statuses: List<String>): JSONObject =
        JSONObject()
            .put("passed", statuses.count { it == "passed" })
            .put("warnings", statuses.count { it == "warning" })
            .put("failed", statuses.count { it == "failed" })
            .put("skipped", statuses.count { it == "skipped" })
            .put("cancelled", statuses.count { it == "cancelled" })

    private fun categoryForCheck(checkId: String): String =
        when (checkId.substringBefore('.')) {
            "engine", "frontend" -> {
                "local_component"
            }

            "physical" -> {
                "physical_network"
            }

            "transport" -> {
                "transport"
            }

            "tunnel" -> {
                "tunnel"
            }

            "protection" -> {
                if (checkId == "protection.recovery_journal") "recovery" else "protection"
            }

            else -> {
                "local_component"
            }
        }

    private fun safeRemediationKey(value: Any?): String? = (value as? String)?.takeIf(REMEDIATION_KEYS::contains)

    private fun safeFailureDetail(value: String): Boolean =
        value.length <= 64 &&
            FAILURE_DETAIL_PREFIXES.any { prefix ->
                value.removePrefix(prefix).let { suffix ->
                    suffix.length < value.length &&
                        suffix.isNotEmpty() &&
                        suffix.all { character -> character in '0'..'9' }
                }
            }

    private fun safeEvidence(value: String): Boolean =
        value in STATIC_EVIDENCE ||
            EVIDENCE_COUNTER_PREFIXES.any { prefix ->
                value.removePrefix(prefix).let { suffix ->
                    suffix.length < value.length &&
                        suffix.length <= 20 &&
                        suffix.all { character -> character in '0'..'9' }
                }
            }

    private fun stringMap(value: Any?): Map<String, Any?>? {
        val source = value as? Map<*, *> ?: return null
        if (source.keys.any { key -> key !is String }) return null
        return source.entries.associate { (key, entryValue) -> (key as String) to entryValue }
    }

    private fun ZipOutputStream.writeEntry(
        name: String,
        contents: ByteArray,
    ) {
        putNextEntry(ZipEntry(name).apply { time = 0L })
        write(contents)
        closeEntry()
    }

    private fun safeCounter(value: Any?): Long = ((value as? Number)?.toLong() ?: 0L).coerceIn(0L, Long.MAX_VALUE)

    private fun safeEnum(
        value: Any?,
        allowed: Set<String>,
        fallback: String?,
    ): String? = (value as? String)?.lowercase()?.takeIf(allowed::contains) ?: fallback

    private fun sha256(bytes: ByteArray): String =
        MessageDigest
            .getInstance("SHA-256")
            .digest(bytes)
            .joinToString("") { byte -> "%02x".format(byte) }

    private val CONNECTION_PHASES =
        setOf(
            "disconnected",
            "preparing",
            "connectingh3",
            "connectingh2",
            "connected",
            "degraded",
            "reconnecting",
            "disconnecting",
            "error",
        )
    private const val MAX_DIAGNOSTIC_FINDINGS = 64
    private const val MAX_EVIDENCE_ITEMS = 16
    private const val MAX_TIMELINE_EVENTS = 256
    private val SESSION_ID =
        Regex("^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
    private val SESSION_STATES =
        setOf("pending", "running", "cancelling", "completed", "failed", "cancelled")
    private val DIAGNOSTIC_MODES = setOf("standard", "deep")
    private val CHECK_STATUSES =
        setOf("pending", "running", "passed", "warning", "failed", "skipped", "cancelled")
    private val SEVERITIES = setOf("info", "warning", "error", "critical")
    private val TRANSPORTS = setOf("h2", "h3", "http2", "http3")
    private val ADDRESS_FAMILIES = setOf("ipv4", "ipv6", "dual")
    private val TRANSPORT_STAGES =
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
    private val EVENT_TYPES =
        setOf(
            "attempt_started",
            "endpoint_resolved",
            "socket_connected",
            "tls_ready",
            "quic_ready",
            "masque_accepted",
            "peer_settings_received",
            "address_assigned",
            "tunnel_ready",
            "first_packet_sent",
            "first_packet_received",
            "fallback_started",
            "reconnect_scheduled",
            "network_changed",
            "recovery_probe_started",
            "recovery_probe_succeeded",
            "recovery_probe_failed",
            "path_promoted",
            "queue_saturated",
            "disconnected",
            "failed",
        )
    private val CHECK_IDS =
        setOf(
            "engine.control_channel",
            "engine.event_stream",
            "engine.capabilities",
            "engine.configuration",
            "engine.secure_storage_metadata",
            "frontend.socks_port",
            "frontend.http_port",
            "frontend.system_proxy_state",
            "physical.network_present",
            "physical.ipv4_route",
            "physical.ipv6_route",
            "physical.dns_available",
            "physical.network_generation",
            "transport.h3_connect",
            "transport.h3_datagram",
            "transport.h2_tcp",
            "transport.h2_tls",
            "transport.h2_connect",
            "transport.endpoint_pin",
            "transport.fallback_policy",
            "tunnel.address_assignment",
            "tunnel.routes",
            "tunnel.dns",
            "tunnel.first_packet",
            "tunnel.ipv4_egress",
            "tunnel.ipv6_egress",
            "protection.kill_switch",
            "protection.dns_path",
            "protection.route_ownership",
            "protection.recovery_journal",
        )
    private val FAILURE_CODES =
        setOf(
            "ENGINE_UNAVAILABLE",
            "AGENT_UNREACHABLE",
            "VPN_SERVICE_UNAVAILABLE",
            "PROXY_PORT_IN_USE",
            "PHYSICAL_IPV4_UNAVAILABLE",
            "PHYSICAL_IPV6_UNAVAILABLE",
            "PHYSICAL_DNS_UNAVAILABLE",
            "PHYSICAL_NETWORK_CHANGED",
            "H3_UDP_UNREACHABLE",
            "H3_HANDSHAKE_TIMEOUT",
            "H3_PROTOCOL_ERROR",
            "H3_DATAGRAM_UNAVAILABLE",
            "H3_CONNECTION_CLOSED",
            "PMTU_REVALIDATION_EXHAUSTED",
            "H2_TCP_CONNECT_FAILED",
            "H2_TLS_FAILED",
            "H2_STREAM_CLOSED",
            "H2_CONNECT_REJECTED",
            "H2_GOAWAY",
            "ALL_TRANSPORTS_FAILED",
            "ENDPOINT_PIN_MISMATCH",
            "IDENTITY_INVALID",
            "AUTHENTICATION_FAILED",
            "CONFIGURATION_INVALID",
            "CONNECT_IP_REJECTED",
            "ADDRESS_ASSIGNMENT_INVALID",
            "TUN_ADDRESS_MISSING",
            "SOCKET_PROTECTION_FAILED",
            "SOCKET_AFFINITY_INVALID",
            "DNS_APPLY_FAILED",
            "ROUTE_APPLY_FAILED",
            "KILL_SWITCH_APPLY_FAILED",
            "KILL_SWITCH_STATE_MISMATCH",
            "SYSTEM_PROXY_STATE_MISMATCH",
            "ROUTE_RESTORE_INCOMPLETE",
            "DNS_RESTORE_INCOMPLETE",
            "SYSTEM_PROXY_STALE",
            "PLATFORM_RECOVERY_PENDING",
            "PACKET_SEND_FAILED",
            "PACKET_SEND_TIMEOUT",
            "PACKET_RECEIVE_FAILED",
            "PACKET_RECEIVE_STALLED",
            "SEND_QUEUE_FULL",
            "DIAGNOSTIC_ALREADY_RUNNING",
            "DIAGNOSTIC_TIMEOUT",
            "DIAGNOSTIC_CANCELLED",
            "DIAGNOSTIC_DEPENDENCY_FAILED",
            "INTERNAL",
        )
    private val REMEDIATION_KEYS =
        setOf(
            "none",
            "retry",
            "try_http2",
            "check_physical_network",
            "refresh_or_replace_identity",
            "replace_identity",
            "review_configuration",
            "restore_platform_state",
            "resolve_dependency",
            "run_deep_diagnostics",
            "run_release_leak_gate",
            "inspect_platform_state",
            "generate_tunnel_traffic",
            "export_diagnostics",
        )
    private val DURATION_METRICS =
        setOf(
            "last_connect_duration_milliseconds",
            "last_h3_handshake_duration_milliseconds",
            "last_h2_handshake_duration_milliseconds",
        )
    private val COUNTER_METRICS =
        setOf(
            "reconnect_count",
            "fallback_count",
            "network_change_count",
            "send_queue_high_watermark",
            "send_queue_drop_count",
        )
    private val FAILURE_DETAIL_PREFIXES =
        setOf("attempt ", "status ", "generation ", "queue depth ")
    private val STATIC_EVIDENCE =
        setOf(
            "network=present",
            "network=absent",
            "kill_switch=active",
            "kill_switch=inactive",
            "kill_switch=notApplicable",
            "kill_switch=unknown",
        )
    private val EVIDENCE_COUNTER_PREFIXES =
        setOf("dns_server_count=", "generation=")
    private val SERVICE_STATES = setOf("running", "stopped", "unknown")
    private val PROCESS_STATES = setOf("reachable", "unreachable", "unknown")
    private val NOTIFICATION_STATES = setOf("active", "inactive", "unknown")
    private val RUNTIME_STATES = setOf("running", "stopped", "unknown")
    private val KILL_SWITCH_STATES = setOf("active", "inactive", "notapplicable", "error")
}
