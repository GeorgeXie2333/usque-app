package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionNetworkRecoveryTest {
    private class Harness {
        val events = mutableListOf<String>()
        val stops = ArrayDeque<(Boolean) -> Unit>()
        val scheduled = ArrayDeque<() -> Unit>()
        val delays = mutableListOf<Long>()
        val recovery =
            SessionNetworkRecovery(
                suspendSession = { events.add("suspend") },
                stop = { callback ->
                    events.add("stop")
                    stops.add(callback)
                },
                schedule = { delay, action ->
                    delays.add(delay)
                    scheduled.add(action)
                },
                restart = { events.add("restart") },
                cleanupFailed = { events.add("cleanup_failed") },
            )

        fun fail(
            generation: Long = 1L,
            online: Boolean = true,
            retryable: Boolean = true,
        ): Boolean = recovery.failed(retryable, generation, online)

        fun flush() {
            while (scheduled.isNotEmpty()) scheduled.removeFirst().invoke()
        }

        fun restartAfterFailure() {
            fail()
            stops.removeFirst()(true)
            flush()
        }
    }

    @Test
    fun networkEventsCannotActivateRecovery() {
        val h = Harness()
        assertFalse(h.recovery.networkChanged(1L, true))
        assertEquals(emptyList<String>(), h.events)
        assertFalse(h.recovery.active)
    }

    @Test
    fun retryableExitSuspendsAndConfirmsStopBeforeOneReplacement() {
        val h = Harness()
        assertTrue(h.fail())
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
        assertEquals(emptyList<Long>(), h.delays)
        h.stops.removeFirst()(true)
        assertEquals(listOf(250L), h.delays)
        h.flush()
        assertEquals(listOf("suspend", "stop", "restart"), h.events)
        assertTrue(h.recovery.active)
    }

    @Test
    fun offlineWaitPreservesIntentWithoutRepeatedCleanupOrHandshakes() {
        val h = Harness()
        h.fail(online = false)
        h.stops.removeFirst()(true)
        repeat(5) { h.recovery.networkChanged(2L, false) }
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
        assertTrue(h.recovery.active)
        h.recovery.networkChanged(3L, true)
        h.flush()
        assertEquals(listOf("suspend", "stop", "restart"), h.events)
    }

    @Test
    fun repeatedFailureWhileStoppingDoesNotOverlapCleanup() {
        val h = Harness()
        repeat(5) { h.fail() }
        h.recovery.networkChanged(2L, true)
        h.recovery.networkChanged(3L, true)
        assertEquals(listOf("suspend", "stop"), h.events)
        h.stops.removeFirst()(true)
        repeat(5) { h.fail(generation = 3L) }
        h.flush()
        assertEquals(listOf(250L), h.delays)
        assertEquals(1, h.events.count { it == "restart" })
    }

    @Test
    fun replacementNetworkFailuresBackOffAndCapAtThirtySeconds() {
        val h = Harness()
        h.restartAfterFailure()
        repeat(9) { h.restartAfterFailure() }
        assertEquals(
            listOf(250L, 1_000L, 2_000L, 4_000L, 8_000L, 15_000L, 30_000L, 30_000L, 30_000L, 30_000L),
            h.delays,
        )
        assertEquals(10, h.events.count { it == "stop" })
        assertEquals(10, h.events.count { it == "restart" })
    }

    @Test
    fun duplicateGenerationCannotResetBackoffOrScheduleAnotherTimer() {
        val h = Harness()
        h.restartAfterFailure()
        h.fail()
        h.stops.removeFirst()(true)
        repeat(5) { h.recovery.networkChanged(1L, true) }
        assertEquals(listOf(250L, 1_000L), h.delays)
        h.flush()
        assertEquals(2, h.events.count { it == "restart" })
    }

    @Test
    fun newUsableGenerationReplacesPendingBackoffWithSettleDelay() {
        val h = Harness()
        h.restartAfterFailure()
        h.fail()
        h.stops.removeFirst()(true)
        val oldTimer = h.scheduled.removeFirst()
        h.recovery.networkChanged(2L, true)
        oldTimer()
        assertEquals(1, h.events.count { it == "restart" })
        assertEquals(listOf(250L, 1_000L, 250L), h.delays)
        h.flush()
        assertEquals(2, h.events.count { it == "restart" })
        h.fail(generation = 2L)
        h.stops.removeFirst()(true)
        assertEquals(1_000L, h.delays.last())
    }

    @Test
    fun lossRevokesTimerAndRapidNetworkChangesCoalesce() {
        val h = Harness()
        h.fail()
        h.stops.removeFirst()(true)
        h.recovery.networkChanged(2L, true)
        h.recovery.networkChanged(3L, true)
        h.recovery.networkChanged(4L, false)
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
        h.recovery.networkChanged(5L, true)
        repeat(5) { h.recovery.networkChanged(5L, true) }
        h.flush()
        assertEquals(1, h.events.count { it == "restart" })
    }

    @Test
    fun networkChangeDuringStartupRevokesWorkerBeforeStoppingReplacement() {
        val h = Harness()
        h.restartAfterFailure()
        h.recovery.networkChanged(2L, true)
        h.recovery.networkChanged(3L, true)
        h.flush()
        assertEquals(listOf("suspend", "stop", "restart", "suspend", "stop"), h.events)
        h.stops.removeFirst()(true)
        assertEquals(listOf(250L, 250L), h.delays)
        h.flush()
        assertEquals(2, h.events.count { it == "restart" })
    }

    @Test
    fun staleNetworkEventCannotMakeOfflineSessionUsable() {
        val h = Harness()
        h.fail(generation = 3L, online = false)
        h.stops.removeFirst()(true)
        h.recovery.networkChanged(2L, true)
        h.recovery.networkChanged(3L, true)
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
    }

    @Test
    fun manualCancelRevokesStopResultsAndTimers() {
        for (stopped in listOf(false, true)) {
            val h = Harness()
            h.fail()
            val complete = h.stops.removeFirst()
            if (stopped) complete(true)
            h.recovery.cancel()
            if (!stopped) complete(true)
            h.flush()
            assertEquals(listOf("suspend", "stop"), h.events)
            assertFalse(h.recovery.active)
            assertFalse(h.recovery.networkChanged(2L, true))
        }
    }

    @Test
    fun terminalFailureNeverStartsOrRetainsRecovery() {
        val inactive = Harness()
        assertFalse(inactive.fail(retryable = false))
        assertEquals(emptyList<String>(), inactive.events)
        for (stopped in listOf(false, true)) {
            val h = Harness()
            h.fail()
            val complete = h.stops.removeFirst()
            if (stopped) complete(true)
            assertFalse(h.fail(retryable = false))
            if (!stopped) complete(true)
            h.flush()
            assertEquals(listOf("suspend", "stop"), h.events)
            assertFalse(h.recovery.active)
        }
    }

    @Test
    fun lateStopCallbackCannotAuthorizeOrFailNewRecoveryOwner() {
        for (oldResult in listOf(false, true)) {
            val h = Harness()
            h.fail()
            val oldStop = h.stops.removeFirst()
            h.recovery.cancel()
            h.fail(generation = 2L)
            oldStop(oldResult)
            h.flush()
            assertEquals(listOf("suspend", "stop", "suspend", "stop"), h.events)
            h.stops.removeFirst()(true)
            h.flush()
            assertEquals(1, h.events.count { it == "restart" })
        }
    }

    @Test
    fun duplicateStopCallbackCannotOverrideConfirmedCleanup() {
        val h = Harness()
        h.fail()
        val complete = h.stops.removeFirst()
        complete(true)
        complete(false)
        h.flush()
        assertEquals(listOf("suspend", "stop", "restart"), h.events)
        assertTrue(h.recovery.active)
    }

    @Test
    fun cleanupFailureCancelsIntentAndNeverStartsReplacement() {
        val h = Harness()
        h.fail()
        h.stops.removeFirst()(false)
        h.recovery.networkChanged(2L, true)
        h.flush()
        assertEquals(listOf("suspend", "stop", "cleanup_failed"), h.events)
        assertFalse(h.recovery.active)
    }

    @Test
    fun connectedClearsRecoveryAndResetsBackoffForNextEstablishedExit() {
        val h = Harness()
        repeat(3) { h.restartAfterFailure() }
        h.recovery.connected()
        assertFalse(h.recovery.active)
        h.restartAfterFailure()
        assertEquals(listOf(250L, 1_000L, 2_000L, 250L), h.delays)
    }

    @Test
    fun establishedChainRebuildsOnlyAfterConfirmedCleanup() {
        val h = Harness()
        assertTrue(h.recovery.networkChanged(1L, true, restartEstablishedSession = true))
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
        h.stops.removeFirst()(true)
        h.flush()
        assertEquals(listOf("suspend", "stop", "restart"), h.events)
        assertEquals(listOf(250L), h.delays)
        h.recovery.connected()
        assertFalse(h.recovery.active)
    }

    @Test
    fun inactiveOrdinarySessionConsumesCallbacksWithoutRebuilding() {
        val h = Harness()
        assertFalse(h.recovery.networkChanged(1L, true))
        assertFalse(h.recovery.networkChanged(1L, true, restartEstablishedSession = true))
        assertFalse(h.recovery.networkChanged(0L, true, restartEstablishedSession = true))
        assertEquals(emptyList<String>(), h.events)
        assertTrue(h.recovery.networkChanged(2L, true, restartEstablishedSession = true))
        assertEquals(listOf("suspend", "stop"), h.events)
    }

    @Test
    fun establishedChainLossWaitsOfflineAndCoalescesLaterChanges() {
        val h = Harness()
        h.recovery.networkChanged(1L, false, restartEstablishedSession = true)
        h.stops.removeFirst()(true)
        repeat(5) { h.recovery.networkChanged(2L, false) }
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
        h.recovery.networkChanged(3L, true)
        h.recovery.networkChanged(4L, true)
        h.recovery.networkChanged(5L, false)
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
        h.recovery.networkChanged(6L, true)
        h.flush()
        assertEquals(1, h.events.count { it == "restart" })
        assertEquals(1, h.events.count { it == "stop" })
    }

    @Test
    fun runtimeErrorBeforeNetworkCallbackKeepsOneRecoveryOwner() {
        val h = Harness()
        h.fail(generation = 1L)
        h.recovery.networkChanged(2L, true, restartEstablishedSession = true)
        repeat(5) { h.recovery.networkChanged(2L, true, restartEstablishedSession = true) }
        h.flush()
        assertEquals(listOf("suspend", "stop"), h.events)
        h.stops.removeFirst()(true)
        h.flush()
        assertEquals(listOf("suspend", "stop", "restart"), h.events)
        assertEquals(listOf(250L), h.delays)
    }

    @Test
    fun networkCallbackBeforeRuntimeErrorStillRetriesFailedReplacement() {
        val h = Harness()
        h.recovery.networkChanged(2L, true, restartEstablishedSession = true)
        h.fail(generation = 2L)
        h.stops.removeFirst()(true)
        h.flush()
        h.fail(generation = 2L)
        h.stops.removeFirst()(true)
        h.flush()
        assertEquals(listOf("suspend", "stop", "restart", "suspend", "stop", "restart"), h.events)
        assertEquals(listOf(250L, 1_000L), h.delays)
        assertTrue(h.recovery.active)
    }

    @Test
    fun replacementNativeFailureWhileOfflineWaitsForNewUsableGeneration() {
        val h = Harness()
        h.recovery.networkChanged(1L, true, restartEstablishedSession = true)
        h.stops.removeFirst()(true)
        h.flush()
        h.fail(generation = 2L, online = false)
        h.stops.removeFirst()(true)
        h.flush()
        assertEquals(1, h.events.count { it == "restart" })
        assertTrue(h.recovery.active)
        h.recovery.networkChanged(2L, true)
        h.flush()
        assertEquals(1, h.events.count { it == "restart" })
        h.recovery.networkChanged(3L, true)
        h.flush()
        assertEquals(2, h.events.count { it == "restart" })
        assertEquals(listOf(250L, 250L), h.delays)
    }

    @Test
    fun supersededFinalHandoffCannotClearNewRecoveryOwner() {
        val events = mutableListOf<String>()
        val stops = ArrayDeque<(Boolean) -> Unit>()
        val scheduled = ArrayDeque<() -> Unit>()
        val handoffs = ArrayDeque<() -> Unit>()
        var workerRevision = 0L
        lateinit var recovery: SessionNetworkRecovery
        recovery =
            SessionNetworkRecovery(
                suspendSession = {
                    workerRevision++
                    events.add("suspend")
                },
                stop = { callback ->
                    events.add("stop")
                    stops.add(callback)
                },
                schedule = { _, action -> scheduled.add(action) },
                restart = {
                    events.add("restart")
                    val worker = workerRevision
                    // Model the service's generation check before final TUN publication.
                    handoffs.add {
                        if (workerRevision == worker) {
                            events.add("handoff")
                            recovery.connected()
                        }
                    }
                },
                cleanupFailed = { events.add("cleanup_failed") },
            )
        recovery.networkChanged(1L, true, restartEstablishedSession = true)
        stops.removeFirst()(true)
        scheduled.removeFirst()()
        val staleHandoff = handoffs.removeFirst()
        recovery.networkChanged(2L, true)
        staleHandoff()
        assertTrue(recovery.active)
        assertEquals(listOf("suspend", "stop", "restart", "suspend", "stop"), events)
        stops.removeFirst()(true)
        scheduled.removeFirst()()
        handoffs.removeFirst()()
        assertFalse(recovery.active)
        assertEquals(1, events.count { it == "handoff" })
        assertEquals(2, events.count { it == "restart" })
    }

    @Test
    fun duplicateGenerationCannotRebuildSuccessfullyReplacedChain() {
        val h = Harness()
        h.recovery.networkChanged(1L, true, restartEstablishedSession = true)
        h.stops.removeFirst()(true)
        h.flush()
        h.recovery.connected()
        repeat(5) {
            assertFalse(h.recovery.networkChanged(1L, true, restartEstablishedSession = true))
        }
        assertEquals(1, h.events.count { it == "stop" })
        h.recovery.networkChanged(2L, true, restartEstablishedSession = true)
        assertEquals(2, h.events.count { it == "stop" })
    }

    @Test
    fun manualCancelAndTerminalFailureConsumeLateNetworkCallbacks() {
        for (terminal in listOf(false, true)) {
            val h = Harness()
            h.recovery.networkChanged(1L, true, restartEstablishedSession = true)
            val complete = h.stops.removeFirst()
            if (terminal) h.fail(generation = 2L, retryable = false) else h.recovery.cancel()
            complete(true)
            h.flush()
            assertFalse(h.recovery.networkChanged(1L, true, restartEstablishedSession = true))
            if (terminal) {
                assertFalse(h.recovery.networkChanged(2L, true, restartEstablishedSession = true))
            }
            assertFalse(h.recovery.active)
            assertEquals(listOf("suspend", "stop"), h.events)
        }
    }

    @Test
    fun chainCleanupFailureCannotReauthorizeSameGeneration() {
        val h = Harness()
        h.recovery.networkChanged(1L, true, restartEstablishedSession = true)
        h.stops.removeFirst()(false)
        assertFalse(h.recovery.networkChanged(1L, true, restartEstablishedSession = true))
        h.flush()
        assertEquals(listOf("suspend", "stop", "cleanup_failed"), h.events)
        assertFalse(h.recovery.active)
    }

    @Test
    fun staleTimerCannotAuthorizeNewRecoveryOwnerOnSameNetwork() {
        val h = Harness()
        h.fail()
        h.stops.removeFirst()(true)
        val staleTimer = h.scheduled.removeFirst()
        h.recovery.cancel()
        h.fail()
        staleTimer()
        assertEquals(listOf("suspend", "stop", "suspend", "stop"), h.events)
        h.stops.removeFirst()(true)
        h.flush()
        assertEquals(1, h.events.count { it == "restart" })
    }
}
