package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RoutingSettingsErrorTest {
    @Test fun nativeValidationKeepsOnlyCodesAndIds() {
        val id = "00112233-4455-4677-8899-aabbccddeeff"
        for (token in listOf(
            "ROUTING_RULE_CONFLICT:$id:$id",
            "ROUTING_RULE_INVALID:$id",
            "ROUTING_RULE_LIMIT",
            "ROUTING_UPGRADE_REQUIRED",
        )) {
            assertEquals(token, RoutingSettingsError.nativeFailure("JNI exception: $token private.example"))
            assertEquals(token, RoutingSettingsError.fromWire(token))
        }
        assertNull(RoutingSettingsError.fromWire("ROUTING_RULE_INVALID:private.example"))
        assertNull(RoutingSettingsError.fromWire("ROUTING_RULE_INVALID:$id:private.example"))
        assertEquals("NETWORK_SETTINGS_UNCONFIRMED", RoutingSettingsError.nativeFailure("private.example failed"))
        assertEquals("NETWORK_SETTINGS_SAVE_FAILED", RoutingSettingsError.nativeFailure("NETWORK_SETTINGS_SAVE_FAILED"))
    }

    @Test fun routingCapabilityRequiresAnExplicitBoolean() {
        assertEquals(true, NetworkQualityFields.capabilities("""{"routing_rules":true}""")["routing_rules"])
        for (json in listOf(null, "{}", """{"routing_rules":"true"}""", """{"routing_rules":false}""")) {
            assertEquals(false, NetworkQualityFields.capabilities(json)["routing_rules"])
        }
    }
}
