package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class MasqueConnectionSettingsTest {
    @Test
    fun explicitIntegerLimitsSurviveTheNativeCapabilityDocument() {
        assertEquals(6, MasqueConnectionSettings.maximum(expandedNativeCapabilities()))
        for (count in 1..6) {
            assertEquals(count, MasqueConnectionSettings.maximum("""{"max_masque_connections":$count}"""))
        }
    }

    @Test
    fun missingMalformedOrCoercedLimitsRemainUnsupported() {
        for (value in listOf(null, "", "invalid", "{}", "[]", """{"max_masque_connections":6""")) {
            assertEquals(0, MasqueConnectionSettings.maximum(value))
        }
        for (value in listOf("0", "-1", "7", "255", "4294967296", "6.0", "6.1", "\"6\"", "true", "null", "[]", "{}")) {
            assertEquals(0, MasqueConnectionSettings.maximum("""{"max_masque_connections":$value}"""))
        }
    }

    @Test
    fun oversizedUtf8DocumentsCannotAdvertiseAConnectionLimit() {
        val value = JSONObject(expandedNativeCapabilities()).put("padding", "界".repeat(6000)).toString()
        assertEquals(0, MasqueConnectionSettings.maximum(value))
    }
}
