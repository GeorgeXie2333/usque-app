package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class L4PerformanceFieldsTest {
    @Test
    fun nestedPerformanceIsStrictBoundedAndOptional() {
        assertFalse(L4StatusFields.decode("{}")!!.containsKey("performance"))
        val wait =
            JSONObject()
                .put(
                    "samples",
                    0,
                ).put("sum_us", 0)
                .put("max_us", 0)
                .put("buckets", JSONArray(List(32) { 0 }))
        val perf =
            JSONObject()
                .put("h3_read_bytes", 32768)
                .put("udp_receive_buffer_bytes", JSONObject.NULL)
                .put("target", "private-address")
                .put("tcp_write_calls", -1)
                .put("tcp_partial_writes", 1.5)
                .put("udp_buffer_source", "private-address")
                .put("command_wait", wait)
        val decoded =
            JSONObject(
                L4StatusFields.decode(JSONObject().put("performance", perf).toString())!!,
            ).getJSONObject("performance")
        assertEquals(32768L, decoded.getLong("h3_read_bytes"))
        assertFalse(decoded.has("udp_receive_buffer_bytes"))
        assertFalse(decoded.has("tcp_write_calls"))
        assertFalse(decoded.has("tcp_partial_writes"))
        assertFalse(decoded.toString().contains("private-address"))
        assertEquals(32, decoded.getJSONObject("command_wait").getJSONArray("buckets").length())
        wait.put("buckets", JSONArray(List(33) { 0 }))
        val malformed =
            JSONObject(
                L4StatusFields.decode(JSONObject().put("performance", perf).toString())!!,
            ).getJSONObject("performance")
        assertFalse(malformed.has("command_wait"))
        assertNull(L4StatusFields.decode(" ".repeat(L4StatusFields.MAX_JSON_BYTES + 1)))
    }

    @Test
    fun statusJsonLimitCountsUtf8BytesIncludingMultibyteAndSurrogatePairs() {
        val limit = L4StatusFields.MAX_JSON_BYTES
        val prefix = "{\"unused\":\""
        val suffix = "\"}"
        val remaining = limit - prefix.length - suffix.length
        val ascii = prefix + "a".repeat(remaining) + suffix
        assertEquals(limit, ascii.toByteArray(Charsets.UTF_8).size)
        assertNotNull(L4StatusFields.decode(ascii))
        assertNull(L4StatusFields.decode(prefix + "a".repeat(remaining + 1) + suffix))

        val unicode = prefix + "汉".repeat(remaining / 3) + "a".repeat(remaining % 3) + suffix
        assertEquals(limit, unicode.toByteArray(Charsets.UTF_8).size)
        assertNotNull(L4StatusFields.decode(unicode))
        for ((text, bytesPerUnit) in listOf("汉" to 3, "\uD834\uDD1E" to 4)) {
            val oversized = prefix + text.repeat(remaining / bytesPerUnit + 1) + suffix
            assertTrue(oversized.length < limit)
            assertTrue(oversized.toByteArray(Charsets.UTF_8).size > limit)
            assertNull(L4StatusFields.decode(oversized))
            assertNull(L4StatusFields.encode(JSONObject(oversized)))
        }
    }

    @Test
    fun buildInfoAlsoUsesUtf8ByteLimit() {
        val oversized = JSONObject().put("unused", "汉".repeat(400)).toString()
        assertTrue(oversized.length < 1024)
        assertTrue(oversized.toByteArray(Charsets.UTF_8).size > 1024)
        assertNull(L4StatusFields.buildInfo(oversized))
    }

    @Test
    fun buildTypeIsExplicitAndNeverGuessedFromVersion() {
        assertNull(L4StatusFields.buildInfo(null))
        val value =
            L4StatusFields.buildInfo(
                """{"version":"0.2.5","architecture":"aarch64","debug_assertions":false,"secret":"private"}""",
            )!!
        assertFalse(value.getBoolean("debug_assertions"))
        assertEquals("aarch64", value.getString("architecture"))
        assertFalse(value.has("secret"))
        assertFalse(L4StatusFields.buildInfo("""{"version":"0.2.5"}""")!!.has("debug_assertions"))
        assertTrue(L4StatusFields.buildInfo("""{"debug_assertions":true}""")!!.getBoolean("debug_assertions"))
    }

    @Test
    fun historicalBuildExperimentsStayReadableAndMissingRemainsUnknown() {
        assertFalse(L4StatusFields.buildInfo("{}")!!.has("network_experiment"))
        assertFalse(L4StatusFields.buildInfo("""{"network_experiment":"private-value"}""")!!.has("network_experiment"))
        for (value in listOf(
            "none",
            "android_l4_portable_recv_only",
            "android_l4_rcvbuf_control",
            "android_l4_rcvbuf_2m",
            "android_h3_rcvbuf_control",
            "android_h3_rcvbuf_2m",
        )) {
            assertEquals(
                value,
                L4StatusFields
                    .buildInfo(
                        JSONObject().put("network_experiment", value).toString(),
                    )!!
                    .getString("network_experiment"),
            )
        }
    }

    @Test
    fun receiveHistoryIsBoundedAndCannotExportSocketOrTargetIdentifiers() {
        val interval =
            JSONObject()
                .put(
                    "elapsed_ms",
                    1000,
                ).put(
                    "interval_ms",
                    1000,
                ).put(
                    "path_reset",
                    false,
                ).put(
                    "h3_read_bytes",
                    4000,
                ).put(
                    "tcp_accepted_bytes",
                    4000,
                ).put("tun_ingress_bytes", 60)
                .put("socket_drops", JSONObject.NULL)
                .put("target", "private-address")
        val history = JSONArray(List(120) { interval })
        val receive =
            JSONObject()
                .put(
                    "requested_buffer_bytes",
                    2097152,
                ).put(
                    "buffer_request_status",
                    "accepted",
                ).put(
                    "overflow_monitoring",
                    "enabled",
                ).put(
                    "receive_backend",
                    "recvmmsg",
                ).put("send_backend", "sendmmsg")
                .put("history", history)
                .put("socket_id", 123)
                .put("secret", "private-address")
        val source = JSONObject().put("performance", JSONObject().put("receive", receive))
        val decoded =
            JSONObject(
                L4StatusFields.decode(source.toString())!!,
            ).getJSONObject("performance").getJSONObject("receive")
        assertEquals(120, decoded.getJSONArray("history").length())
        assertFalse(decoded.has("socket_drops_reported"))
        assertFalse(decoded.has("socket_id"))
        assertFalse(decoded.toString().contains("private-address"))
        receive.put("history", JSONArray(List(121) { interval }))
        assertFalse(JSONObject(L4StatusFields.decode(source.toString())!!).getJSONObject("performance").has("receive"))
    }

    @Test
    fun networkExportOmitsIdentityAndKeepsMissingLossUnknown() {
        val input =
            mapOf(
                "connection_instance_id" to "48a799bc-9d7b-4ff7-9f78-ff2a60299110",
                "target" to "private-address",
                "metrics" to
                    mapOf(
                        "udp_recv_syscall_count" to 20,
                        "udp_datagram_received_count" to 30,
                        "packets_lost" to 0,
                        "local_quic_packets_lost_observed" to null,
                        "local_quic_pto_count_observed" to 1,
                        "secret" to "private-address",
                    ),
            )
        val output = NetworkQualityFields.diagnostic(input, true)!!
        assertFalse(output.has("connection_instance_id"))
        assertFalse(output.toString().contains("private-address"))
        assertFalse(output.getJSONObject("metrics").has("packets_lost"))
        assertTrue(output.getJSONObject("metrics").isNull("local_quic_packets_lost_observed"))
        assertEquals(20, output.getJSONObject("metrics").getInt("udp_recv_syscall_count"))
    }

    @Test
    fun sufficientReceiveBufferExportsTargetWithoutFakingAnotherRequest() {
        val result =
            JSONObject(
                L4StatusFields.receive(
                    JSONObject()
                        .put("buffer_target_bytes", 2097152)
                        .put("buffer_request_status", "already_sufficient")
                        .put("requested_buffer_bytes", JSONObject.NULL)
                        .put("socket_id", 42),
                )!!,
            )
        assertEquals(2097152L, result.getLong("buffer_target_bytes"))
        assertEquals("already_sufficient", result.getString("buffer_request_status"))
        assertFalse(result.has("requested_buffer_bytes"))
        assertFalse(result.has("socket_id"))
        assertFalse(result.has("socket_drops_reported"))
    }

    @Test
    fun h3SocketObservationExportsActualSizeWithoutL4HistoryOrIdentity() {
        val source =
            mapOf(
                "udp_socket_receive" to
                    mapOf(
                        "receive_buffer_bytes" to 4194304,
                        "send_buffer_bytes" to 229376,
                        "socket_id" to 42,
                        "observation" to
                            mapOf(
                                "buffer_target_bytes" to 2097152,
                                "requested_buffer_bytes" to 2097152,
                                "buffer_request_status" to "accepted",
                                "receive_backend" to "recvmmsg",
                                "send_backend" to "sendmmsg",
                                "socket_drops_reported" to null,
                                "history" to emptyList<Any>(),
                                "history_dropped" to 0,
                                "target" to "private-target",
                            ),
                    ),
            )
        val result = NetworkQualityFields.diagnostic(source, false)!!.getJSONObject("udp_socket_receive")
        assertEquals(4194304, result.getInt("receive_buffer_bytes"))
        assertEquals(2097152, result.getJSONObject("observation").getInt("buffer_target_bytes"))
        assertEquals(2097152, result.getJSONObject("observation").getInt("requested_buffer_bytes"))
        assertFalse(result.has("socket_id"))
        assertFalse(result.toString().contains("private-target"))
        assertFalse(result.getJSONObject("observation").has("history"))
        assertFalse(result.getJSONObject("observation").has("socket_drops_reported"))
        assertTrue(NetworkQualityFields.diagnostic(emptyMap<String, Any>(), false)!!.isNull("udp_socket_receive"))
    }
}
