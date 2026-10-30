package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.math.BigDecimal
import java.math.BigInteger
import java.security.MessageDigest
import java.util.zip.ZipInputStream

class AndroidDirectGatewayExportTest {
    @Test
    fun nativeCountersCrossTheServiceWireAndReachTheActualSanitizedArchive() {
        val expected = fixture()
        val direct =
            JSONObject(expected.toString())
                .put("destination", "private.example")
                .put("error", "token=private-secret")
                .put("rule", JSONObject().put("domain", "private.example"))
        val connectionId = "123e4567-e89b-42d3-a456-426614174000"
        val state =
            state(
                JSONObject()
                    .put("direct_gateway", direct)
                    .put("endpoint", "192.0.2.9")
                    .put("connection_instance_id", connectionId),
            )
        assertEquals(
            connectionId,
            requireNotNull(NetworkQualityFields.decode(state.networkQualityJson))["connection_instance_id"],
        )
        val entries = archive(snapshot(state))
        val quality = JSONObject(requireNotNull(entries["network-quality.json"]).toString(Charsets.UTF_8))
        assertCounters(expected, quality.getJSONObject("direct_gateway"))
        assertFalse(quality.has("connection_instance_id"))
        for (bytes in entries.values) {
            val text = bytes.toString(Charsets.UTF_8)
            for (privateText in listOf("private.example", "private-secret", "192.0.2.9")) {
                assertFalse(text.contains(privateText))
            }
        }

        val manifest = JSONObject(requireNotNull(entries["manifest.json"]).toString(Charsets.UTF_8))
        val contents = manifest.getJSONArray("contents")
        assertEquals(entries.size - 1, contents.length())
        for (index in 0 until contents.length()) {
            val entry = contents.getJSONObject(index)
            val bytes = requireNotNull(entries[entry.getString("path")])
            assertEquals(bytes.size, entry.getInt("size"))
            val digest = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
            assertEquals(digest, entry.getString("sha256"))
        }
    }

    @Test
    fun missingGatewayNeverBecomesZeroWhileRegisteredZeroCountersSurviveExport() {
        for (value in listOf(null, JSONObject.NULL, "unavailable", JSONArray())) {
            val quality = exportedQuality(state(JSONObject().put("direct_gateway", value)))
            assertTrue(quality.isNull("direct_gateway"))
        }
        val zeros = fixture()
        for (key in zeros.keys().asSequence().toList()) zeros.put(key, 0L)
        val exported = exportedQuality(state(JSONObject().put("direct_gateway", zeros))).getJSONObject("direct_gateway")
        assertCounters(zeros, exported)
        assertEquals(10, exported.length())
    }

    @Test
    fun directCountersRejectNumericCoercionsAndPreserveTheSignedMaximum() {
        val invalid =
            listOf(
                -1L,
                1.5,
                1.0,
                "7",
                true,
                JSONObject.NULL,
                JSONObject().put("count", 7),
                JSONArray().put(7),
                BigInteger("9223372036854775808"),
                BigInteger("18446744073709551616"),
                BigDecimal("1e100"),
            )
        for (value in invalid) {
            val direct = fixture().put("fallback_total", value)
            val exported =
                exportedQuality(state(JSONObject().put("direct_gateway", direct))).getJSONObject("direct_gateway")
            assertTrue("Rejected $value", exported.isNull("fallback_total"))
            assertEquals(7L, exported.getLong("flow_failures"))
        }
        for (exact in listOf(9_007_199_254_740_993L, Long.MAX_VALUE - 1, Long.MAX_VALUE)) {
            val maximum = fixture()
            for (key in maximum.keys().asSequence().toList()) maximum.put(key, exact)
            val exported =
                exportedQuality(state(JSONObject().put("direct_gateway", maximum))).getJSONObject("direct_gateway")
            assertCounters(maximum, exported)
            for (key in maximum.keys().asSequence()) assertEquals(exact, exported.getLong(key))
        }

        for (raw in listOf("1.0", "-1", "9223372036854775808", "18446744073709551616", "1e100")) {
            val decoded =
                requireNotNull(NetworkQualityFields.decode("{\"direct_gateway\":{\"fallback_total\":$raw}}"))
            assertNull((decoded["direct_gateway"] as Map<*, *>)["fallback_total"])
        }
    }

    @Test
    fun replacementAndTerminalResetCannotExportThePreviousGateway() {
        val state = state(JSONObject().put("direct_gateway", fixture()))
        assertEquals(21L, exportedQuality(state).getJSONObject("direct_gateway").getLong("fallback_total"))
        state.applyNativeSnapshot(JSONObject().put("phase", "connected").put("network_quality", JSONObject()))
        assertTrue(exportedQuality(state).isNull("direct_gateway"))
        state.applyNativeSnapshot(
            JSONObject()
                .put("phase", "connected")
                .put("network_quality", JSONObject().put("direct_gateway", fixture())),
        )
        state.reset("disconnected")
        assertNull(state.networkQualityJson)
        assertFalse(archive(snapshot(state)).containsKey("network-quality.json"))
        state.applyNativeSnapshot(JSONObject().put("phase", "connectingH3"))
        assertFalse(archive(snapshot(state)).containsKey("network-quality.json"))
    }

    @Test
    fun countersFitAlongsideBoundedHistoryAndTheExistingQualityByteLimitStillApplies() {
        val queues =
            listOf(
                "tunToTransport",
                "proxyToTransport",
                "transportOutgoing",
                "h3DatagramSend",
                "h3WireSend",
                "transportToTun",
                "transportToProxy",
                "directDns",
            )
        val quality =
            JSONObject()
                .put("direct_gateway", fixture())
                .put(
                    "queues",
                    JSONArray(
                        queues.map { kind ->
                            JSONObject().put("kind", kind).put("current_items", 0).put(
                                "backpressure",
                                JSONObject().put("waits", 1).put("buckets", JSONArray(List(32) { 1L })),
                            )
                        },
                    ),
                ).put(
                    "samples",
                    JSONArray(
                        (1..16).map { sequence ->
                            JSONObject()
                                .put("sequence", sequence)
                                .put("sampled_at_unix_ms", 1000L * sequence)
                                .put("monotonic_millis", 1000L * sequence)
                                .put("downloaded_bytes", 123456L)
                                .put("uploaded_bytes", 654321L)
                                .put("rtt_ms", 42)
                                .put("loss_basis_points", 0)
                        },
                    ),
                ).put(
                    "transport_performance",
                    JSONObject()
                        .put("h2_batch_sizes", JSONArray(List(7) { 1L }))
                        .put("h3_batch_sizes", JSONArray(List(7) { 1L })),
                )
        val state = state(quality)
        val encoded = requireNotNull(state.networkQualityJson)
        assertTrue(encoded.toByteArray(Charsets.UTF_8).size <= NetworkQualityFields.MAX_JSON_BYTES)
        val exported = exportedQuality(state)
        assertEquals(16, exported.getJSONArray("samples").length())
        assertEquals(8, exported.getJSONArray("queues").length())
        assertCounters(fixture(), exported.getJSONObject("direct_gateway"))
        val encodedBytes = encoded.toByteArray(Charsets.UTF_8).size
        val padded = encoded + " ".repeat(NetworkQualityFields.MAX_JSON_BYTES - encodedBytes)
        assertTrue(requireNotNull(NetworkQualityFields.decode(padded)).containsKey("direct_gateway"))
        assertNull(NetworkQualityFields.decode("$padded "))
        assertFalse(archive(mapOf("network_quality" to "$padded ")).containsKey("network-quality.json"))
    }

    @Test
    fun bundleLimitIsCheckedBeforeReadingPlatformMetadataOrOpeningTheProvider() {
        assertThrows(IOException::class.java) {
            AndroidMaintenance.writeDiagnosticArchive(
                snapshot = emptyMap(),
                diagnosticSession = null,
                connectionTimeline = emptyMap(),
                capturedLogs = AndroidLogStore.Snapshot("x".repeat(8 * 1024 * 1024), emptyMap()),
                metadata = { error("Platform metadata must not be read for an oversized archive") },
                openOutput = { error("The provider must not be opened for an oversized archive") },
            )
        }
    }

    private fun assertCounters(
        expected: JSONObject,
        actual: JSONObject,
    ) {
        val keys = expected.keys().asSequence().toSet()
        assertEquals(keys, actual.keys().asSequence().toSet())
        for (key in keys) {
            val value = actual.get(key)
            assertTrue("$key must remain an integer", value is Int || value is Long)
            assertEquals(expected.getLong(key), actual.getLong(key))
        }
    }

    private fun fixture(): JSONObject {
        val root =
            generateSequence(File(requireNotNull(System.getProperty("user.dir"))).absoluteFile) { it.parentFile }
                .first { File(it, "apps/usque_gui/pubspec.yaml").isFile }
        return JSONObject(File(root, "crates/usque-android/tests/fixtures/android_direct_gateway.json").readText())
    }

    private fun state(quality: JSONObject): ServiceSnapshotState =
        ServiceSnapshotState().apply {
            applyNativeSnapshot(JSONObject().put("phase", "connected").put("network_quality", quality))
        }

    private fun snapshot(state: ServiceSnapshotState): Map<String, Any?> {
        val wire =
            state.wireEntries(
                ServiceSnapshotState.PlatformFlags(
                    tunnelOpen = state.phase == "connected",
                    activeMode = "vpn",
                    platformLockdown = false,
                    alwaysOn = false,
                ),
            )
        // The same JSON bridge used by VpnControlClient's Messenger decoder.
        return mapOf(
            "phase" to state.phase,
            "network_quality" to
                NetworkQualityFields.decode(wire[ServiceSnapshotState.WireKeys.NETWORK_QUALITY] as? String),
        )
    }

    private fun exportedQuality(state: ServiceSnapshotState): JSONObject =
        JSONObject(requireNotNull(archive(snapshot(state))["network-quality.json"]).toString(Charsets.UTF_8))

    private fun archive(snapshot: Map<String, Any?>): Map<String, ByteArray> {
        val output = ByteArrayOutputStream()
        var metadataRead = false
        var opened = false
        AndroidMaintenance.writeDiagnosticArchive(
            snapshot = snapshot,
            diagnosticSession = null,
            connectionTimeline = emptyMap(),
            capturedLogs = AndroidLogStore.Snapshot("", emptyMap()),
            metadata = {
                metadataRead = true
                AndroidMaintenance.DiagnosticArchiveMetadata("0.0.0-test", false, null, 36, listOf("arm64-v8a"))
            },
            openOutput = {
                assertTrue(metadataRead)
                assertFalse(opened)
                opened = true
                output
            },
        )
        assertTrue(opened)
        val entries = linkedMapOf<String, ByteArray>()
        ZipInputStream(output.toByteArray().inputStream()).use { zip ->
            while (true) {
                val entry = zip.nextEntry ?: break
                assertFalse(entries.containsKey(entry.name))
                entries[entry.name] = zip.readBytes()
            }
        }
        return entries
    }
}
