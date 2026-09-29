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
        logSnapshot: AndroidLogStore.Snapshot? = null,
    ) {
        val capturedLogs =
            logSnapshot?.let { AndroidLogStore.fromMap(it.toMap()) } ?: AndroidLogStore.readPersisted(context)
        val logs = capturedLogs.lines
        val capture = DiagnosticMetadata.captureSummary(snapshot, connectionTimeline)
        val scopedTimeline =
            if (capture["timeline_scope_availability"] == "stale") {
                connectionTimeline +
                    mapOf("events" to emptyList<Any>(), "metrics" to emptyMap<String, Any>(), "availability" to "stale")
            } else {
                connectionTimeline
            }
        val connection =
            JSONObject()
                .put("phase", safeEnum(snapshot["phase"], CONNECTION_PHASES, "unknown"))
                .put("transport", safeEnum(snapshot["transport"], setOf("h2", "h3"), null))
                .put("data_plane", L4StatusFields.mode(snapshot["data_plane"]))
                .put(
                    "l4",
                    L4StatusFields
                        .decode(
                            (snapshot["l4"] as? Map<*, *>)?.let {
                                JSONObject(it).toString()
                            },
                        )?.let { JSONObject(it) },
                ).put(
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
            sanitizeConnectionTimeline(scopedTimeline).toString(2).toByteArray()
        payloads["export-capture.json"] = JSONObject(capture).toString(2).toByteArray()
        payloads["platform-health.json"] = platformHealth.toString(2).toByteArray()
        NetworkQualityFields.diagnostic(snapshot["network_quality"], snapshot["data_plane"] == "l4_proxy")?.let {
            payloads["network-quality.json"] = it.toString(2).toByteArray()
        }
        val sanitizedSession =
            diagnosticSession?.let {
                sanitizeDiagnosticSession(
                    it,
                    DiagnosticMetadata.connectionId(snapshot),
                    DiagnosticMetadata.unsigned(snapshot["network_generation"]),
                )
            }
        if (diagnosticSession != null) {
            payloads["diagnostic-session.json"] =
                requireNotNull(sanitizedSession).toString(2).toByteArray()
        }
        if (logs.isNotEmpty()) payloads["logs/android-engine.jsonl"] = logs.toByteArray()
        payloads["log-storage-health.json"] = JSONObject(capturedLogs.health).toString(2).toByteArray()
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
                .put(
                    "app_debuggable",
                    context.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE != 0,
                ).put("native_build", L4StatusFields.buildInfo(NativeEngine.buildInfo()) ?: JSONObject.NULL)
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
        check(AndroidLocaleController.clear(context)) {
            "Android locale state could not be cleared"
        }
        AndroidLogStore.clearPersisted(context)
        FlagSvgCache(context).clear()
        AndroidPolicyStore.clear(context)
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

    internal fun sanitizeDiagnosticSession(
        source: Map<String, Any?>,
        expectedConnectionId: String? = null,
        expectedNetworkGeneration: Long? = null,
    ): JSONObject {
        val startedAt = safeCounter(source["started_at_unix_milliseconds"])
        val state = safeEnum(source["state"], SESSION_STATES, "failed") ?: "failed"
        val mode = safeEnum(source["mode"], DIAGNOSTIC_MODES, "standard") ?: "standard"
        val findings = JSONArray()
        val statuses = mutableListOf<String>()
        var currentCheck: String? = null
        val rawFindings = source["findings"] as? List<*> ?: emptyList<Any?>()
        for (rawFinding in rawFindings.take(MAX_DIAGNOSTIC_FINDINGS)) {
            val finding = stringMap(rawFinding) ?: continue
            val checkId = (finding["check_id"] as? String)?.takeIf(CHECK_IDS::contains) ?: continue
            val observation = DiagnosticMetadata.observation(finding["observation"])
            val scoped = observation?.get("source") in setOf("runtime", "platform", "active_probe")
            val observedId = observation?.get("connection_instance_id") as? String
            val observedGeneration = DiagnosticMetadata.unsigned(observation?.get("network_generation"))
            val mismatched =
                scoped && (
                    (expectedConnectionId != null && observedId != null && expectedConnectionId != observedId) ||
                        (
                            expectedNetworkGeneration != null && observedGeneration != null &&
                                expectedNetworkGeneration != observedGeneration
                        )
                )
            val status =
                if (mismatched) {
                    "skipped"
                } else {
                    safeEnum(finding["status"], CHECK_STATUSES, "skipped")
                        ?: "skipped"
                }
            statuses += status
            if (status == "running" && currentCheck == null) currentCheck = checkId
            val output =
                JSONObject()
                    .put("check_id", checkId)
                    .put("category", categoryForCheck(checkId))
                    .put("status", status)
                    .put(
                        "severity",
                        if (mismatched) "info" else safeEnum(finding["severity"], SEVERITIES, "info") ?: "info",
                    ).put("duration_milliseconds", safeCounter(finding["duration_milliseconds"]))
            val expectedSummary = "diagnostics.$checkId.$status"
            if (finding["summary_key"] == expectedSummary) {
                output.put("summary_key", expectedSummary)
            }
            (finding["summary_key"] as? String)
                ?.takeIf(
                    NETWORK_SUMMARIES::contains,
                )?.let { output.put("summary_key", it) }
            (if (mismatched) "nq_retry" else safeRemediationKey(finding["remediation_key"]))?.let { key ->
                output.put("remediation_key", key)
            }
            val evidence = JSONArray()
            (if (mismatched) emptyList<Any>() else finding["sanitized_evidence"] as? List<*>)
                ?.asSequence()
                ?.filterIsInstance<String>()
                ?.filter(::safeEvidence)
                ?.take(MAX_EVIDENCE_ITEMS)
                ?.forEach(evidence::put)
            output.put("sanitized_evidence", evidence)
            observation?.let {
                output.put(
                    "observation",
                    JSONObject(if (mismatched) it + ("availability" to "stale") else it),
                )
            }
            val typedEvidence = JSONArray()
            (if (mismatched) emptyList<Any>() else finding["evidence"] as? List<*>)
                ?.mapNotNull(DiagnosticMetadata::evidence)
                ?.take(MAX_EVIDENCE_ITEMS)
                ?.forEach { typedEvidence.put(JSONObject(it)) }
            output.put("evidence", typedEvidence)
            val findingStarted = safeCounter(finding["started_at_unix_milliseconds"])
            if (startedAt > 0 && findingStarted >= startedAt) {
                output.put("started_after_milliseconds", findingStarted - startedAt)
            }
            (finding["dependency_reason"] as? String)
                ?.takeIf { it in CHECK_IDS || it in DiagnosticsContract.remediationKeys }
                ?.let { dependency -> output.put("dependency_reason", dependency) }
            if (!mismatched) {
                sanitizeFailure(finding["failure"])?.let { failure ->
                    output.put("failure", failure)
                }
            }
            if (mismatched) output.put("summary_key", "nq_finding_stale")
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
        currentCheck?.let { output.put("current_check", it) }
        val completedAt = safeCounter(source["completed_at_unix_milliseconds"])
        if (startedAt > 0 && completedAt >= startedAt) {
            output.put("completed_after_milliseconds", completedAt - startedAt)
        }
        return output
    }

    internal fun sanitizeConnectionTimeline(
        source: Map<String, Any?>,
        includeLiveTimestamps: Boolean = false,
    ): JSONObject {
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
            if (includeLiveTimestamps) {
                output.put("timestamp_unix_milliseconds", safeCounter(event["timestamp_unix_milliseconds"]))
            }
            safeEnum(event["stage"], TRANSPORT_STAGES, null)?.let { stage ->
                output.put("stage", stage)
            }
            safeEnum(event["transport"], TRANSPORTS, null)?.let { transport ->
                output.put("transport", transport)
            }
            safeEnum(event["address_family"], ADDRESS_FAMILIES, null)?.let { family ->
                output.put("address_family", family)
            }
            safeEnum(
                event["queue_kind"],
                setOf(
                    "tun_to_transport",
                    "proxy_to_transport",
                    "transport_outgoing",
                    "h3_datagram_send",
                    "h3_wire_send",
                    "transport_to_tun",
                    "transport_to_proxy",
                    "direct_dns",
                ),
                null,
            )?.let {
                output.put("queue_kind", it)
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
        if (includeLiveTimestamps) {
            metrics.put("current_smoothed_rtt_known", metricsSource["current_smoothed_rtt_known"] == true)
        }
        for (key in DURATION_METRICS) {
            (metricsSource[key] as? Number)?.let { value ->
                metrics.put(key, safeCounter(value))
            }
        }
        for (key in COUNTER_METRICS) {
            (metricsSource[key] as? Number)?.let { value ->
                metrics.put(key, safeCounter(value))
            }
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
        val result =
            JSONObject()
                .put("schema_version", 1)
                .put("events", events)
                .put("metrics", metrics)
                .put("dropped_event_count", safeCounter(source["dropped_event_count"]))
        DiagnosticMetadata.runtimeId(source["connection_instance_id"])?.let { result.put("connection_instance_id", it) }
        DiagnosticMetadata.unsigned(source["session_generation"])?.let { result.put("session_generation", it) }
        (source["retained"] as? Boolean)?.let { result.put("retained", it) }
        (source["source"] as? String)?.takeIf { it in setOf("runtime", "platform") }?.let { result.put("source", it) }
        (source["availability"] as? String)
            ?.takeIf {
                it in setOf("observed", "inferred", "unavailable", "stale")
            }?.let { result.put("availability", it) }
        if (includeLiveTimestamps) {
            DiagnosticMetadata.unsigned(source["captured_at_unix_milliseconds"])?.let {
                result.put("captured_at_unix_milliseconds", it)
            }
        } else {
            DiagnosticMetadata.unsigned(source["captured_at_unix_milliseconds"])?.let {
                result.put("capture_age_milliseconds", (System.currentTimeMillis() - it).coerceAtLeast(0L))
            }
        }
        return result
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

            "transport", "quality" -> {
                "transport"
            }

            "tunnel" -> {
                "tunnel"
            }

            "protection", "dns" -> {
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

    private fun safeEvidence(value: String): Boolean = DiagnosticMetadata.fromLegacy(value) != null

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
    private val TRANSPORT_STAGES = DiagnosticMetadata.transportStages
    private val EVENT_TYPES = DiagnosticsContract.eventTypes
    private val CHECK_IDS = DiagnosticsContract.checkIds
    private val FAILURE_CODES = DiagnosticsContract.failureCodes
    private val REMEDIATION_KEYS = DiagnosticsContract.remediationKeys
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
    private val NETWORK_SUMMARIES = DiagnosticsContract.summaryKeys
    private val SERVICE_STATES = setOf("running", "stopped", "unknown")
    private val PROCESS_STATES = setOf("reachable", "unreachable", "unknown")
    private val NOTIFICATION_STATES = setOf("active", "inactive", "unknown")
    private val RUNTIME_STATES = setOf("running", "stopped", "unknown")
    private val KILL_SWITCH_STATES = setOf("active", "inactive", "notapplicable", "error")
}
