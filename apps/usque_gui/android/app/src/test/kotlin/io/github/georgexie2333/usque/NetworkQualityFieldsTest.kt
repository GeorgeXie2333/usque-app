package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.Executor
import java.util.concurrent.atomic.AtomicInteger

class NetworkQualityFieldsTest {
    @Test fun qualityBridgeIsBoundedTypedAndOmitsUnknownPrivateFields() {
        val source =
            JSONObject()
                .put(
                    "sampled_at_unix_ms",
                    1000,
                ).put("connection_instance_id", "12345678-1234-4234-8234-123456789012")
                .put("endpoint", "private.example")
                .put("ssid", "private-ssid")
                .put(
                    "metrics",
                    JSONObject()
                        .put(
                            "latest_rtt_milliseconds",
                            7,
                        ).put("latest_rtt_availability", "available")
                        .put("packets_lost", -1)
                        .put("server", "private.example"),
                ).put(
                    "queues",
                    JSONArray((0..40).map { JSONObject().put("kind", "h3WireSend").put("current_items", 4) }),
                ).put("migration", JSONObject().put("last_reason_code", "192.0.2.9").put("phase_code", "probing"))
                .put(
                    "direct_dns",
                    JSONObject().put("mode", "doh").put("server_name", "private.example").put("phase_code", "ready"),
                )
        val encoded = requireNotNull(NetworkQualityFields.encode(source))
        assertFalse(encoded.contains("private"))
        assertFalse(encoded.contains("192.0.2.9"))
        val decoded = requireNotNull(NetworkQualityFields.decode(encoded))
        assertEquals(1, (decoded["queues"] as List<*>).size)
        val metrics = decoded["metrics"] as Map<*, *>
        assertEquals(7L, metrics["latest_rtt_milliseconds"])
        assertEquals("available", metrics["latest_rtt_availability"])
        assertNull(metrics["packets_lost"])
        assertNull(NetworkQualityFields.decode("x".repeat(16 * 1024 + 1)))
        assertTrue(NetworkQualityFields.capabilities(null).values.none { it })
        assertTrue(NetworkQualityFields.capabilities("{\"network_quality\":true}").getValue("network_quality"))
        assertFalse(NetworkQualityFields.capabilities("{\"network_quality\":\"true\"}").getValue("network_quality"))
    }

    @Test fun standardHasNoProbeCallsAndNoSnapshotMutation() {
        val calls = AtomicInteger()
        val source =
            mapOf<String, Any?>(
                "phase" to "connected",
                "direct_dns_mode" to "doh",
                "direct_dns_configuration" to "valid",
            )
        val before = source.toMap()
        val doctor =
            AndroidDiagnosticsCoordinator(executor = Executor(Runnable::run), networkProbe = {
                id,
                _,
                ->
                calls.incrementAndGet()
                NetworkDiagnosticChecks.probe(id, null)
            })
        doctor.start("standard", source, true, true, true, true)
        assertEquals(0, calls.get())
        assertEquals(before, source)
        val findings = doctor.current()!!["findings"] as List<*>
        assertEquals(39, findings.size)
        val configuration =
            findings.filterIsInstance<Map<*, *>>().single {
                it["check_id"] ==
                    "dns.direct_encrypted_configuration"
            }
        assertEquals("passed", configuration["status"])
        assertEquals("nq_finding_dns_custom_valid", configuration["summary_key"])
    }

    @Test fun h2UnavailableLossAndStaleNumbersNeverPass() {
        val source =
            mapOf<String, Any?>(
                "transport" to "h2",
                "network_quality" to
                    mapOf(
                        "connection_instance_id" to "id",
                        "sampled_at_unix_ms" to 1000L,
                        "metrics" to
                            mapOf("interval_loss_availability" to "unsupported", "interval_loss_basis_points" to 0L),
                    ),
            )
        assertEquals("skipped", NetworkDiagnosticChecks.evaluate("quality.packet_loss", source, 1000)["status"])
        assertEquals("warning", NetworkDiagnosticChecks.evaluate("quality.rtt", source, 5000)["status"])
        assertEquals(
            "skipped",
            NetworkDiagnosticChecks.evaluate("transport.migration_capability", source, 1000)["status"],
        )
    }

    @Test fun probeOutputNeverCopiesRawRemoteErrors() {
        val result =
            NetworkDiagnosticChecks.probe(
                "dns.direct_encrypted_reachability",
                "{\"code\":\"failed\",\"error\":\"private.example 192.0.2.1\"}",
            )
        assertEquals("failed", result["status"])
        assertFalse(result.toString().contains("private.example"))
        assertFalse(result.toString().contains("192.0.2.1"))
    }
}
