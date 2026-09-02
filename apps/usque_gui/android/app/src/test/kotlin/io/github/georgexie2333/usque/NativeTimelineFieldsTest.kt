package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.Executor

class NativeTimelineFieldsTest {
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
}
