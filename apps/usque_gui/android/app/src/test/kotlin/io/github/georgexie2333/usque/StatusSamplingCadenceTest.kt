package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class StatusSamplingCadenceTest {
    @Test
    fun oneShotDelaySubtractsWorkAndSkipsOverdueDeadlines() {
        val cadence = StatusSamplingCadence(1_000)
        assertTrue(cadence.takeDue(250))
        assertEquals(800L, cadence.delayUntilNext(450))
        assertTrue(cadence.takeDue(1_250))
        assertEquals(980L, cadence.delayUntilNext(1_270))
        assertEquals(500L, cadence.delayUntilNext(7_750))
        assertTrue(cadence.takeDue(8_250))
        assertEquals(900L, cadence.delayUntilNext(8_350))
    }

    @Test
    fun snapshotWorkDoesNotAccumulateIntoTheNextDeadline() {
        val cadence = StatusSamplingCadence(1_000)
        assertTrue(cadence.takeDue(250))
        assertFalse(cadence.takeDue(500))
        assertTrue(cadence.takeDue(1_250))
        assertTrue(cadence.takeDue(2_280))
        assertTrue(cadence.takeDue(3_249))
        assertFalse(cadence.takeDue(3_251))
    }

    @Test
    fun delayedWorkSkipsMissedBeatsWithoutBurstSampling() {
        val cadence = StatusSamplingCadence(1_000)
        assertTrue(cadence.takeDue(0))
        assertTrue(cadence.takeDue(5_450))
        repeat(5) { assertFalse(cadence.takeDue(5_450L + it)) }
        assertFalse(cadence.takeDue(5_900))
        assertTrue(cadence.takeDue(6_000))
    }
}
