package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ChainNetworkRecoveryTest {
    private class Harness {
        val events = mutableListOf<String>()
        val stops = ArrayDeque<(Boolean) -> Unit>()
        val scheduled = ArrayDeque<() -> Unit>()
        val recovery =
            ChainNetworkRecovery(
                suspendSession = { events.add("suspend") },
                stop = { callback ->
                    events.add("stop")
                    stops.add(callback)
                },
                schedule = { delay, action ->
                    assertEquals(250L, delay)
                    scheduled.add(action)
                },
                restart = { events.add("restart") },
                cleanupFailed = { events.add("cleanup_failed") },
            )

        fun flush() {
            while (scheduled.isNotEmpty()) scheduled.removeFirst().invoke()
        }
    }

    @Test
    fun ordinaryWarpKeepsItsNativeRecovery() {
        val h = Harness()
        assertFalse(h.recovery.networkChanged(chainRunning = false, networkPresent = true))
        assertEquals(emptyList<String>(), h.events)
    }

    @Test
    fun establishedChainStopsBeforeStartingOneReplacement() {
        val h = Harness()
        assertTrue(h.recovery.networkChanged(chainRunning = true, networkPresent = true))
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
        h.stops.removeFirst()(true)
        h.flush()
        assertEquals(listOf("suspend", "stop", "restart"), h.events)
        assertTrue(h.recovery.active)
        h.recovery.cancel() // Replacement has completed final TUN handoff.
        assertFalse(h.recovery.active)
    }

    @Test
    fun offlineWaitDoesNotStartHandshakesOrStopRepeatedly() {
        val h = Harness()
        h.recovery.networkChanged(true, false)
        h.stops.removeFirst()(true)
        repeat(5) { h.recovery.networkChanged(false, false) }
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
        h.recovery.networkChanged(false, true)
        h.flush()
        assertEquals(listOf("suspend", "stop", "restart"), h.events)
    }

    @Test
    fun rapidChangesCoalesceAndLossRevokesAScheduledRestart() {
        val h = Harness()
        h.recovery.networkChanged(true, true)
        h.stops.removeFirst()(true)
        repeat(5) { h.recovery.networkChanged(false, true) }
        h.recovery.networkChanged(false, false)
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
        repeat(5) { h.recovery.networkChanged(false, true) }
        h.flush()
        assertEquals(1, h.events.count { it == "restart" })
    }

    @Test
    fun changeDuringHandshakeStopsThatAttemptBeforeItsSuccessor() {
        val h = Harness()
        h.recovery.networkChanged(true, true)
        h.stops.removeFirst()(true)
        h.flush()
        h.recovery.networkChanged(false, true)
        h.flush()
        assertEquals(listOf("suspend", "stop", "restart", "suspend", "stop"), h.events)
        h.stops.removeFirst()(true)
        h.flush()
        assertEquals(2, h.events.count { it == "restart" })
    }

    @Test
    fun disconnectOrManualRetryRevokesBothStopAndTimerCallbacks() {
        for (stopped in listOf(false, true)) {
            val h = Harness()
            h.recovery.networkChanged(true, true)
            val complete = h.stops.removeFirst()
            if (stopped) complete(true)
            h.recovery.cancel()
            if (!stopped) complete(true)
            h.flush()
            assertEquals(listOf("suspend", "stop"), h.events)
            assertFalse(h.recovery.active)
        }
    }

    @Test
    fun oldStopResultCannotAuthorizeOrFailANewSession() {
        for (oldResult in listOf(false, true)) {
            val h = Harness()
            h.recovery.networkChanged(true, true)
            val oldStop = h.stops.removeFirst()
            h.recovery.cancel()
            h.recovery.networkChanged(true, true)
            oldStop(oldResult)
            h.flush()
            assertEquals(listOf("suspend", "stop", "suspend", "stop"), h.events)
            h.stops.removeFirst()(true)
            h.flush()
            assertEquals(1, h.events.count { it == "restart" })
        }
    }

    @Test
    fun unconfirmedCleanupNeverStartsAReplacementAndCanBeRetriedInProcess() {
        val h = Harness()
        h.recovery.networkChanged(true, true)
        h.stops.removeFirst()(false)
        h.flush()
        assertEquals(listOf("suspend", "stop", "cleanup_failed"), h.events)
        assertFalse(h.recovery.active)
        // Model a manual connection succeeding, followed by another change.
        h.recovery.networkChanged(true, true)
        h.stops.removeFirst()(true)
        h.flush()
        assertEquals(1, h.events.count { it == "restart" })
    }
}
