package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ControlServiceStartTest {
    @Test
    fun generationNRetainCannotStopGenerationNPlusOneConnection() {
        assertEquals(
            ControlServiceStart.Decision.RETAIN_REPLACEMENT,
            ControlServiceStart.decide(4, 5, true, true, true, false, false, false),
        )
        assertEquals(
            ControlServiceStart.Decision.RETAIN_REPLACEMENT,
            ControlServiceStart.decide(4, 5, true, false, false, false, false, false),
        )
    }

    @Test
    fun cancelledConnectionStillOwnsItsPendingCleanupAndRetainedTunnel() {
        assertEquals(
            ControlServiceStart.Decision.RETAIN_REPLACEMENT,
            ControlServiceStart.decide(4, 5, false, false, true, true, false, true),
        )
        assertEquals(
            ControlServiceStart.Decision.RETAIN_REPLACEMENT,
            ControlServiceStart.decide(4, 5, false, false, false, false, false, true),
        )
        assertEquals(
            ControlServiceStart.Decision.STOP_IDLE,
            ControlServiceStart.decide(4, 5, false, false, false, false, false, false),
        )
    }

    @Test
    fun foregroundAdmissionFailureOccursBeforeAnyCallerMutation() {
        val state = mutableMapOf("profile" to "retained", "tunnel" to "owned", "intent" to "existing")
        val before = state.toMap()
        val accepted =
            ControlServiceStart.requestOwnership(false) {
                throw IllegalStateException("foreground admission rejected")
            }
        if (accepted) state["intent"] = "replacement"
        assertFalse(accepted)
        assertEquals(before, state)
        assertTrue(
            ControlServiceStart.requestOwnership(true) { error("An existing started owner needs no new admission") },
        )
    }
}
