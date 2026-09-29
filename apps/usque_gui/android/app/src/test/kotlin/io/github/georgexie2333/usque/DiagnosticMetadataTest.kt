package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class DiagnosticMetadataTest {
    private val runtimeId = "123e4567-e89b-42d3-a456-426614174000"

    @Test
    fun unknownObservationContextIsOmittedInsteadOfZeroOrPrivateText() {
        val metadata =
            requireNotNull(
                DiagnosticMetadata.observation(
                    mapOf(
                        "source" to "runtime",
                        "availability" to "unavailable",
                        "network_generation" to null,
                        "age_milliseconds" to -1,
                        "connection_instance_id" to "account-private",
                        "ssid" to "private",
                    ),
                ),
            )
        assertEquals(mapOf("source" to "runtime", "availability" to "unavailable"), metadata)
        assertNull(DiagnosticMetadata.observation(mapOf("source" to "private-source", "availability" to "observed")))
        val observed =
            requireNotNull(
                DiagnosticMetadata.observation(
                    mapOf(
                        "source" to "platform",
                        "availability" to "observed",
                        "network_generation" to 0L,
                    ),
                ),
            )
        assertEquals(0L, observed["network_generation"])
    }

    @Test
    fun typedEvidenceRejectsUnknownKeysTokensMixedValuesAndFractionalNumbers() {
        assertNull(DiagnosticMetadata.evidence(mapOf("key" to "hostname", "token" to "private.example")))
        assertNull(DiagnosticMetadata.evidence(mapOf("key" to "rtt_ms", "number" to 1.5)))
        assertNull(DiagnosticMetadata.evidence(mapOf("key" to "rtt_ms", "number" to 1L, "token" to "network_present")))
        assertEquals(
            mapOf("key" to "network_generation", "number" to 4L),
            DiagnosticMetadata.fromLegacy("generation=4"),
        )
        assertEquals(
            mapOf("key" to "fact", "token" to "network_present"),
            DiagnosticMetadata.fromLegacy("network=present"),
        )
        assertNull(DiagnosticMetadata.fromLegacy("network=private"))
        assertNull(DiagnosticMetadata.fromLegacy("rtt_ms=-1"))
        assertNull(DiagnosticMetadata.fromLegacy("rtt_ms=18446744073709551615"))
    }

    @Test
    fun captureIdentityUsesActualUuidAndKeepsObserverAndServiceGenerationsDistinct() {
        val capture =
            DiagnosticMetadata.captureSummary(
                mapOf("connection_instance_id" to runtimeId, "connection_generation" to 2L),
                mapOf("connection_instance_id" to runtimeId, "session_generation" to 99L),
            )
        assertEquals("observed", capture["timeline_scope_availability"])
        assertEquals(99L, capture["timeline_session_generation"])
        assertFalse(capture.containsKey("network_generation"))
        val missing = DiagnosticMetadata.captureSummary(emptyMap(), mapOf("connection_instance_id" to runtimeId))
        assertEquals("unavailable", missing["timeline_scope_availability"])
        assertFalse(missing.containsKey("connection_instance_id"))
        val old =
            DiagnosticMetadata.captureSummary(
                mapOf("connection_instance_id" to "223e4567-e89b-42d3-a456-426614174000"),
                mapOf("connection_instance_id" to runtimeId, "events" to listOf(mapOf("sequence" to 1L))),
            )
        assertEquals("stale", old["timeline_scope_availability"])
        assertEquals(1L, old["omitted_mismatched_event_count"])
        assertTrue(DiagnosticsContract.evidenceKeys.contains("network_generation"))
    }
}
