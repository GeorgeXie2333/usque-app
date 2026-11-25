package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Test

class RoutingLimitSettingsTest {
    @Test
    fun nativeLimitsReachFlutter() {
        assertEquals(
            mapOf("max_routing_rules_per_kind" to 4096, "max_routing_target_bytes" to 384 * 1024),
            RoutingLimitSettings.capabilities(expandedNativeCapabilities()),
        )
    }

    @Test
    fun missingMalformedOrOutOfRangeLimitsReportZero() {
        val zero = mapOf("max_routing_rules_per_kind" to 0, "max_routing_target_bytes" to 0)
        for (value in listOf(null, "", "invalid", "{}", "[]")) {
            assertEquals(zero, RoutingLimitSettings.capabilities(value))
        }
        for (limit in listOf("0", "-1", "65537", "4294967296", "4096.5", "\"4096\"", "true", "null")) {
            val reported = RoutingLimitSettings.capabilities("""{"max_routing_rules_per_kind":$limit}""")
            assertEquals(0, reported["max_routing_rules_per_kind"])
        }
        assertEquals(
            0,
            RoutingLimitSettings.capabilities("""{"max_routing_target_bytes":67108865}""")["max_routing_target_bytes"],
        )
    }
}
