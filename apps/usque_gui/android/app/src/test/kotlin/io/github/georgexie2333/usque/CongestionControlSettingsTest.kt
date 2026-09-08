package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class CongestionControlSettingsTest {
    @Test
    fun allFourCapabilitiesAreExplicitAndMissingIsUnsupported() {
        assertEquals(emptyList<String>(), CongestionControlSettings.capabilities(null))
        assertEquals(emptyList<String>(), CongestionControlSettings.capabilities("{}"))
        assertEquals(
            listOf("cubic", "reno", "bbr", "bbr3"),
            CongestionControlSettings.capabilities(
                """{"h3_congestion_control_algorithms":["cubic","reno","bbr","bbr3","unknown","bbr3"]}""",
            ),
        )
        assertNull(CongestionControlSettings.token("BBR3"))
    }

    @Test
    fun newSessionUsesAllSavedFieldsAndRejectsMissingAccounts() {
        val old = """{"id":"a","mtu":1280,"congestion_control":"reno"}"""
        val desired = """{"id":"a","mtu":1400,"congestion_control":"bbr3"}"""
        val fresh = JSONObject(NetworkSettingsFields.savedProfile(old, """{"profiles":[$desired]}"""))
        assertEquals("bbr3", fresh.getString("congestion_control"))
        assertEquals(1400, fresh.getInt("mtu"))
        assertThrows(IllegalStateException::class.java) {
            NetworkSettingsFields.savedProfile(old, """{"profiles":[]}""")
        }
    }

    @Test
    fun snapshotCarriesOnlyKnownSessionNames() {
        val fields =
            ServiceSnapshotState.fromNativeJson(
                JSONObject("""{"phase":"connected","session_congestion_control":"bbr3"}"""),
            )
        assertEquals("bbr3", fields.sessionCongestionControl)
        val unknown =
            ServiceSnapshotState.fromNativeJson(
                JSONObject("""{"phase":"connected","session_congestion_control":"unknown"}"""),
            )
        assertNull(unknown.sessionCongestionControl)
        val snapshot = ServiceSnapshotState()
        snapshot.applyNativeSnapshot(fields)
        assertEquals("bbr3", snapshot.sessionCongestionControl)
        snapshot.reset("disconnected")
        assertNull(snapshot.sessionCongestionControl)
    }
}
