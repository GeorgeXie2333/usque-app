package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidMaintenanceSanitizationTest {
    @Test
    fun missingPlatformObservationNeverBecomesFalseOrZeroInExport() {
        val disconnected = mapOf("phase" to "disconnected", "platform_state_observed" to false)
        for (snapshot in listOf(emptyMap(), disconnected)) {
            val health = AndroidMaintenance.sanitizePlatformHealth(snapshot)
            assertEquals("unavailable", health.getJSONObject("observation").getString("availability"))
            for (key in listOf(
                "tun_fd_valid",
                "tun_interface_present",
                "underlying_network_present",
                "underlying_family_mask",
                "network_generation",
                "dns_server_count",
                "always_on_state",
                "lockdown_state",
                "pending_cleanup",
            )) {
                assertFalse("Missing $key must remain unknown", health.has(key))
            }
            val connection = AndroidMaintenance.sanitizeConnectionSummary(snapshot)
            assertEquals("unavailable", connection.getJSONObject("platform_observation").getString("availability"))
            for (key in listOf(
                "reconnect_count",
                "active_listener_count",
                "platform_lockdown",
                "always_on",
                "exit_ipv4_observed",
                "exit_ipv6_observed",
            )) {
                assertFalse("Missing $key must remain unknown", connection.has(key))
            }
        }
    }

    @Test
    fun actualPlatformFalseAndZeroRemainDistinctFromMissingOrInvalidValues() {
        val snapshot =
            mapOf(
                "platform_state_observed" to true,
                "tun_fd_valid" to false,
                "pending_cleanup" to false,
                "always_on" to false,
                "platform_lockdown" to false,
                "network_generation" to 0L,
                "dns_server_count" to 0,
                "underlying_family_mask" to -1,
                "tun_interface_present" to "false",
                "reconnect_count" to 0,
                "active_listeners" to emptyList<String>(),
                "exit_ipv4" to null,
            )
        val health = AndroidMaintenance.sanitizePlatformHealth(snapshot)
        assertEquals("observed", health.getJSONObject("observation").getString("availability"))
        assertFalse(health.getBoolean("tun_fd_valid"))
        assertFalse(health.getBoolean("pending_cleanup"))
        assertEquals(0L, health.getLong("network_generation"))
        assertEquals(0L, health.getLong("dns_server_count"))
        assertFalse(health.has("underlying_family_mask"))
        assertFalse(health.has("tun_interface_present"))
        val connection = AndroidMaintenance.sanitizeConnectionSummary(snapshot)
        assertEquals(0L, connection.getLong("reconnect_count"))
        assertEquals(0, connection.getInt("active_listener_count"))
        assertFalse(connection.getBoolean("platform_lockdown"))
        assertFalse(connection.getBoolean("always_on"))
        assertFalse(connection.getBoolean("exit_ipv4_observed"))
        assertFalse(connection.has("exit_ipv6_observed"))
    }

    @Test
    fun staleFailureCannotSurviveAsCurrentSeverityRemediationOrRunningCheck() {
        val oldId = "123e4567-e89b-42d3-a456-426614174000"
        val currentId = "223e4567-e89b-42d3-a456-426614174000"
        val old =
            mapOf(
                "check_id" to "transport.h3_connect",
                "status" to "running",
                "severity" to "critical",
                "remediation_key" to "try_http2",
                "failure" to mapOf("code" to "H3_HANDSHAKE_TIMEOUT", "stage" to "quic_handshake"),
                "observation" to
                    mapOf("source" to "runtime", "availability" to "observed", "connection_instance_id" to oldId),
            )
        val source =
            mapOf(
                "state" to "running",
                "current_check" to "transport.h3_connect",
                "findings" to listOf(old),
            )
        val stale = AndroidMaintenance.sanitizeDiagnosticSession(source, currentId)
        val masked = stale.getJSONArray("findings").getJSONObject(0)
        assertEquals("skipped", masked.getString("status"))
        assertEquals("info", masked.getString("severity"))
        assertEquals("nq_retry", masked.getString("remediation_key"))
        assertFalse(masked.has("failure"))
        assertFalse(stale.has("current_check"))

        val current =
            mapOf(
                "check_id" to "quality.rtt",
                "status" to "running",
                "observation" to
                    mapOf("source" to "runtime", "availability" to "observed", "connection_instance_id" to currentId),
            )
        val mixed =
            AndroidMaintenance.sanitizeDiagnosticSession(
                source + ("findings" to listOf(old, current)),
                currentId,
            )
        assertEquals("quality.rtt", mixed.getString("current_check"))
        assertEquals(1, mixed.getJSONObject("summary").getInt("skipped"))
    }

    @Test
    fun provenanceAndTypedEvidenceAreRevalidatedAndWrongConnectionFactsAreMasked() {
        val finding =
            mapOf(
                "check_id" to "quality.rtt",
                "status" to "passed",
                "summary_key" to "nq_finding_healthy",
                "sanitized_evidence" to listOf("rtt_ms=8", "private.example"),
                "evidence" to
                    listOf(
                        mapOf("key" to "rtt_ms", "number" to 8L),
                        mapOf("key" to "rtt_ms", "number" to 1.5),
                        mapOf("key" to "hostname", "token" to "private.example"),
                    ),
                "observation" to
                    mapOf(
                        "source" to "runtime",
                        "availability" to "observed",
                        "age_milliseconds" to 2L,
                        "connection_instance_id" to "123e4567-e89b-42d3-a456-426614174000",
                        "network_generation" to 4L,
                        "device_id" to "private-device",
                    ),
            )
        val session =
            mapOf(
                "session_id" to "123e4567-e89b-42d3-a456-426614174000",
                "state" to "completed",
                "findings" to listOf(finding),
            )
        val public = AndroidMaintenance.sanitizeDiagnosticSession(session).getJSONArray("findings").getJSONObject(0)
        assertEquals(1, public.getJSONArray("evidence").length())
        assertEquals(4L, public.getJSONObject("observation").getLong("network_generation"))
        assertFalse(public.toString().contains("private"))
        val stale = AndroidMaintenance.sanitizeDiagnosticSession(session, "223e4567-e89b-42d3-a456-426614174000", 4L)
        val masked = stale.getJSONArray("findings").getJSONObject(0)
        assertEquals("skipped", masked.getString("status"))
        assertEquals("stale", masked.getJSONObject("observation").getString("availability"))
        assertEquals(0, masked.getJSONArray("evidence").length())
        assertEquals(0, masked.getJSONArray("sanitized_evidence").length())
        assertEquals(0, stale.getJSONObject("summary").getInt("passed"))
    }

    @Test
    fun migrationEventsKeepOnlyAllowlistedFields() {
        val events =
            listOf("migration_started", "migration_path_validated", "migration_promoted")
                .mapIndexed { index, event ->
                    mapOf<String, Any?>(
                        "sequence" to index + 1,
                        "elapsed_from_attempt_start_milliseconds" to index,
                        "event_type" to event,
                        "stage" to "packet_send",
                        "connection_id" to "private-cid",
                        "endpoint" to "192.0.2.9",
                    )
                }
        val sanitized = AndroidMaintenance.sanitizeConnectionTimeline(mapOf("events" to events))
        assertEquals(3, sanitized.getJSONArray("events").length())
        assertFalse(sanitized.toString().contains("private-cid"))
        assertFalse(sanitized.toString().contains("192.0.2.9"))
    }

    @Test
    fun diagnosticSessionIsRebuiltFromAnAllowlist() {
        val session =
            mapOf<String, Any?>(
                "session_id" to "123e4567-e89b-42d3-a456-426614174000",
                "state" to "completed",
                "mode" to "deep",
                "started_at_unix_milliseconds" to 1_000L,
                "completed_at_unix_milliseconds" to 1_125L,
                "profile_name" to "private hotel",
                "findings" to
                    listOf(
                        mapOf(
                            "check_id" to "transport.h3_connect",
                            "category" to "private.example",
                            "status" to "failed",
                            "severity" to "error",
                            "summary_key" to "diagnostics.transport.h3_connect.failed",
                            "remediation_key" to "try_http2",
                            "started_at_unix_milliseconds" to 1_025L,
                            "duration_milliseconds" to 9L,
                            "sanitized_evidence" to
                                listOf("network=present", "192.0.2.4", "private.example"),
                            "failure" to
                                mapOf(
                                    "code" to "H3_HANDSHAKE_TIMEOUT",
                                    "stage" to "quic_handshake",
                                    "transport" to "h3",
                                    "address_family" to "ipv6",
                                    "retryable" to true,
                                    "fallback_allowed" to true,
                                    "severity" to "warning",
                                    "remediation_key" to "try_http2",
                                    "sanitized_detail" to "rawsecret",
                                ),
                        ),
                        mapOf(
                            "check_id" to "private.check",
                            "status" to "failed",
                            "token" to "supersecret",
                        ),
                    ),
            )

        val sanitized = AndroidMaintenance.sanitizeDiagnosticSession(session)
        val text = sanitized.toString()

        assertEquals(125L, sanitized.getLong("completed_after_milliseconds"))
        assertEquals(1, sanitized.getJSONArray("findings").length())
        assertTrue(text.contains("H3_HANDSHAKE_TIMEOUT"))
        assertTrue(text.contains("network=present"))
        for (privateValue in listOf("private hotel", "private.example", "192.0.2.4", "rawsecret", "supersecret")) {
            assertFalse(text.contains(privateValue))
        }
        assertFalse(text.contains("started_at_unix_milliseconds"))
    }

    @Test
    fun connectionTimelineIsBoundedAndKeepsOnlyRelativeSafeFields() {
        val events =
            List(300) { index ->
                mapOf<String, Any?>(
                    "sequence" to (index + 1),
                    "timestamp_unix_milliseconds" to 1_000_000L + index,
                    "elapsed_from_attempt_start_milliseconds" to index,
                    "event_type" to "attempt_started",
                    "stage" to "endpoint_resolution",
                    "endpoint" to "private.example",
                    "failure" to
                        if (index == 299) {
                            mapOf(
                                "code" to "H3_UDP_UNREACHABLE",
                                "stage" to "quic_handshake",
                                "retryable" to true,
                                "fallback_allowed" to true,
                                "severity" to "warning",
                                "remediation_key" to "try_http2",
                                "sanitized_detail" to "attempt 2",
                            )
                        } else {
                            null
                        },
                )
            }
        val timeline =
            mapOf<String, Any?>(
                "events" to events,
                "metrics" to
                    mapOf(
                        "reconnect_count" to 3,
                        "current_smoothed_rtt_known" to false,
                        "last_failure_code" to "H3_UDP_UNREACHABLE",
                        "raw_endpoint" to "192.0.2.8",
                    ),
                "dropped_event_count" to 44,
            )

        val sanitized = AndroidMaintenance.sanitizeConnectionTimeline(timeline)
        val text = sanitized.toString()

        assertEquals(256, sanitized.getJSONArray("events").length())
        assertEquals(3L, sanitized.getJSONObject("metrics").getLong("reconnect_count"))
        assertTrue(text.contains("attempt 2"))
        assertFalse(text.contains("timestamp_unix_milliseconds"))
        assertFalse(text.contains("private.example"))
        assertFalse(text.contains("192.0.2.8"))
    }
}
