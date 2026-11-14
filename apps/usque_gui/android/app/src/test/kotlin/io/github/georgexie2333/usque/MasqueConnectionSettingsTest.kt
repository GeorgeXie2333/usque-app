package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class MasqueConnectionSettingsTest {
    @Test
    fun explicitIntegerLimitsSurviveTheNativeCapabilityDocument() {
        assertEquals(16, MasqueConnectionSettings.maximum(expandedNativeCapabilities()))
        for (count in 1..16) {
            assertEquals(count, MasqueConnectionSettings.maximum("""{"max_masque_connections":$count}"""))
        }
    }

    @Test
    fun missingMalformedOrCoercedLimitsRemainUnsupported() {
        for (value in listOf(null, "", "invalid", "{}", "[]", """{"max_masque_connections":16""")) {
            assertEquals(0, MasqueConnectionSettings.maximum(value))
        }
        val invalidLimits =
            listOf("0", "-1", "17", "255", "4294967296", "16.0", "16.1", "\"16\"", "true", "null", "[]", "{}")
        for (value in invalidLimits) {
            assertEquals(0, MasqueConnectionSettings.maximum("""{"max_masque_connections":$value}"""))
        }
    }

    @Test
    fun oversizedUtf8DocumentsCannotAdvertiseAConnectionLimit() {
        val value = JSONObject(expandedNativeCapabilities()).put("padding", "界".repeat(6000)).toString()
        assertEquals(0, MasqueConnectionSettings.maximum(value))
    }
}
