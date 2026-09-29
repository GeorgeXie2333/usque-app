package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.Executor

class NativeTimelineFieldsTest {
    @Test
    fun nativeScopeAndRetainedCaptureMetadataSurviveLiveAndExportBoundaries() {
        val input =
            mapOf(
                "schema_version" to 1,
                "events" to emptyList<Any>(),
                "connection_instance_id" to "123e4567-e89b-42d3-a456-426614174000",
                "session_generation" to 0L,
                "retained" to true,
                "source" to "runtime",
                "availability" to "observed",
                "captured_at_unix_milliseconds" to 1_000L,
                "device_id" to "private-device",
            )
        val decoded = requireNotNull(NativeTimelineFields.decode(JSONObject(input).toString()))
        assertEquals(true, decoded["retained"])
        assertEquals(0L, (decoded["session_generation"] as Number).toLong())
        assertEquals("observed", decoded["availability"])
        val exported = AndroidMaintenance.sanitizeConnectionTimeline(decoded)
        assertFalse(exported.has("captured_at_unix_milliseconds"))
        assertTrue(exported.has("capture_age_milliseconds"))
        assertEquals("runtime", exported.getString("source"))
        assertFalse(exported.toString().contains("private-device"))
    }

    private val native =
        """
        {"schema_version":1,"events":[
          {"sequence":42,"timestamp_unix_milliseconds":1234,"elapsed_from_attempt_start_milliseconds":87,
           "event_type":"migration_promoted","transport":"http3","endpoint":"private.example","qname":"private.example"}
        ],"metrics":{"fallback_count":3,"current_smoothed_rtt_milliseconds":42,"current_smoothed_rtt_known":true},
        "dropped_event_count":9}
        """.trimIndent()

    @Test
    fun nativeEventsAndMetricsReplaceTheLegacyPhaseTimeline() {
        val decoded = requireNotNull(NativeTimelineFields.decode(native))
        val coordinator = AndroidDiagnosticsCoordinator(Executor { it.run() })
        coordinator.observeSnapshot(mapOf("phase" to "connected"))
        coordinator.observeNativeTimeline(decoded)
        val timeline = coordinator.timeline()
        val event = (timeline["events"] as List<*>).single() as Map<*, *>
        assertEquals("migration_promoted", event["event_type"])
        assertEquals(1234L, (event["timestamp_unix_milliseconds"] as Number).toLong())
        assertEquals(3L, ((timeline["metrics"] as Map<*, *>)["fallback_count"] as Number).toLong())
        assertEquals(true, (timeline["metrics"] as Map<*, *>)["current_smoothed_rtt_known"])
        assertFalse(JSONObject(timeline).toString().contains("private.example"))
        val exported = AndroidMaintenance.sanitizeConnectionTimeline(timeline).toString()
        assertFalse(exported.contains("timestamp_unix_milliseconds"))
        assertFalse(exported.contains("private.example"))
    }

    @Test
    fun oldNativeMissingSchemaAndOversizedPayloadUseTheLegacyFallback() {
        assertNull(NativeTimelineFields.decode(null))
        assertNull(NativeTimelineFields.decode("{}"))
        assertNull(NativeTimelineFields.decode("x".repeat(NativeTimelineFields.MAX_JSON_BYTES + 1)))
        val coordinator = AndroidDiagnosticsCoordinator(Executor { it.run() })
        coordinator.observeSnapshot(mapOf("phase" to "connected"))
        coordinator.observeNativeTimeline(null)
        assertTrue((coordinator.timeline()["events"] as List<*>).isNotEmpty())
    }

    @Test
    fun directDnsTransitionsSurviveBothLiveAndExportSanitization() {
        val decoded =
            requireNotNull(
                NativeTimelineFields.decode(
                    """{"schema_version":1,"events":[
                    {"sequence":1,"event_type":"direct_dns_degraded"},
                    {"sequence":2,"event_type":"direct_dns_recovered"}
                    ],"metrics":{}}""",
                ),
            )
        val liveTypes = (decoded["events"] as List<*>).map { (it as Map<*, *>)["event_type"] }
        assertEquals(listOf("direct_dns_degraded", "direct_dns_recovered"), liveTypes)
        val exported = AndroidMaintenance.sanitizeConnectionTimeline(decoded).getJSONArray("events")
        assertEquals(2, exported.length())
        assertEquals("direct_dns_degraded", exported.getJSONObject(0).getString("event_type"))
        assertEquals("direct_dns_recovered", exported.getJSONObject(1).getString("event_type"))
    }

    @Test
    fun unavailableCountersRemainAbsentWhileObservedZeroSurvives() {
        val coordinator = AndroidDiagnosticsCoordinator(Executor(Runnable::run))
        coordinator.observeSnapshot(mapOf("phase" to "connected"))
        val fallback = AndroidMaintenance.sanitizeConnectionTimeline(coordinator.timeline())
        val metrics = fallback.getJSONObject("metrics")
        assertFalse(metrics.has("reconnect_count"))
        assertFalse(metrics.has("fallback_count"))
        assertFalse(metrics.has("send_queue_high_watermark"))
        assertFalse(metrics.has("send_queue_drop_count"))
        assertFalse(metrics.has("network_change_count"))

        val observed =
            AndroidMaintenance
                .sanitizeConnectionTimeline(
                    mapOf("metrics" to mapOf("fallback_count" to 0L, "send_queue_drop_count" to 0L)),
                ).getJSONObject("metrics")
        assertEquals(0L, observed.getLong("fallback_count"))
        assertEquals(0L, observed.getLong("send_queue_drop_count"))
    }
}
