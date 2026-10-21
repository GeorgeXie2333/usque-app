package io.github.georgexie2333.usque

import org.json.JSONObject
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ContinuousConnectionIntentTest {
    @Test
    fun runtimeIntentSurvivesSettingsProfilesAndExplicitFalseWins() {
        val active = """{"continuous":true}"""
        val hot = ContinuousConnectionIntent.inherit("{}", active)
        val cold = ContinuousConnectionIntent.inherit("{}", hot)
        assertTrue(JSONObject(hot).getBoolean("continuous"))
        assertTrue(JSONObject(cold).getBoolean("continuous"))
        assertFalse(ContinuousConnectionIntent.continuous("""{"continuous":false}""", active))
        assertFalse(ContinuousConnectionIntent.continuous("{}", null))
    }

    @Test
    fun acceptedNativeWorkerDoesNotCompleteRecoveryOrReleaseInheritedCapture() {
        for (tunnel in listOf(false, true)) {
            for (phase in listOf("preparing", "connectingH3", "connectingH2", "reconnecting")) {
                assertFalse(ContinuousConnectionIntent.ready(phase, true, tunnel, true, false))
            }
            assertTrue(ContinuousConnectionIntent.ready("connected", true, tunnel, true, false))
            assertTrue(ContinuousConnectionIntent.ready("degraded", true, tunnel, true, false))
            assertFalse(ContinuousConnectionIntent.ready("connected", false, tunnel, true, false))
            assertFalse(ContinuousConnectionIntent.ready("connected", true, tunnel, true, true))
        }
        assertFalse(ContinuousConnectionIntent.ready("connected", true, true, false, false))
        assertTrue(ContinuousConnectionIntent.ready("connected", true, false, false, false))
    }
}
