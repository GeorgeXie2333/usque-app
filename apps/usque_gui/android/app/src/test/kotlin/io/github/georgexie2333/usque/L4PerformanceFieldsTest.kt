package io.github.georgexie2333.usque

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
        assertNull(L4StatusFields.decode(" ".repeat(16385)))
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
}
