package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidMaintenanceSanitizationTest {
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
