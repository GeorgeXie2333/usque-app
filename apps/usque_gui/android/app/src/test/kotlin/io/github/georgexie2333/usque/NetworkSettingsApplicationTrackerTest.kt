package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NetworkSettingsApplicationTrackerTest {
    @Test
    fun snapshotsCannotFinishTheNextSaveBeforeItsRuntimeReply() {
        for (result in listOf("applied", "failed")) {
            val tracker = NetworkSettingsApplicationTracker()
            val first = tracker.begin(7)
            assertTrue(tracker.committed(first, 7, "first"))
            assertNotNull(tracker.runtimeReplied(first, 7))
            assertNotNull(tracker.finish(first, 7))

            val second = tracker.begin(7)
            assertNotEquals(first, second)
            val observed = mutableListOf<String>()
            repeat(3) {
                if (tracker.allowsObservation(7)) observed += "premature observation"
            }
            assertTrue(observed.isEmpty())
            assertTrue(tracker.busy)
            assertEquals(NetworkSettingsApplicationTracker.Phase.PERSISTING, tracker.phase)
            assertNull(tracker.current(7)?.operationId)

            assertTrue(tracker.committed(second, 7, "second"))
            assertFalse(tracker.allowsObservation(7))
            assertNotNull(tracker.runtimeReplied(second, 7))
            assertTrue(tracker.allowsObservation(7))
            val completed = requireNotNull(tracker.finish(second, 7))
            observed += "$result:${completed.operationId}:${completed.generation}"
            assertEquals(listOf("$result:second:7"), observed)
            assertEquals(NetworkSettingsApplicationTracker.Phase.IDLE, tracker.phase)
            assertFalse(tracker.busy)
        }
    }

    @Test
    fun terminalPathsReleaseTheWholeReservationAndRetainItsResultIdentity() {
        val cases =
            listOf(
                "persistence failed" to NetworkSettingsApplicationTracker.Phase.PERSISTING,
                "no runtime target" to NetworkSettingsApplicationTracker.Phase.PERSISTING,
                "session no longer stable" to NetworkSettingsApplicationTracker.Phase.PERSISTING,
                "runtime rejected" to NetworkSettingsApplicationTracker.Phase.AWAITING_OBSERVATION,
                "runtime failed" to NetworkSettingsApplicationTracker.Phase.AWAITING_OBSERVATION,
                "recovery preference commit failed" to NetworkSettingsApplicationTracker.Phase.AWAITING_OBSERVATION,
            )
        for ((reason, phase) in cases) {
            val tracker = NetworkSettingsApplicationTracker()
            val token = tracker.begin(9)
            if (phase == NetworkSettingsApplicationTracker.Phase.AWAITING_OBSERVATION) {
                assertTrue(tracker.committed(token, 9, "operation"))
                assertNotNull(tracker.runtimeReplied(token, 9))
            }
            val completed = requireNotNull(tracker.finish(token, 9))
            assertEquals(reason, phase, completed.phase)
            assertEquals(reason, token, completed.token)
            assertEquals(reason, 9L, completed.generation)
            if (phase == NetworkSettingsApplicationTracker.Phase.AWAITING_OBSERVATION) {
                assertEquals(reason, "operation", completed.operationId)
            }
            assertFalse(reason, tracker.busy)
            assertNull(reason, tracker.current(9))
            assertEquals(reason, NetworkSettingsApplicationTracker.Phase.IDLE, tracker.phase)
            assertNull(reason, tracker.finish(token, 9))
            val next = tracker.begin(9)
            assertFalse(reason, tracker.allowsObservation(9))
            assertTrue(reason, tracker.owns(next, 9))
            assertNull(reason, tracker.current(9)?.operationId)
        }
    }

    @Test
    fun duplicateOrOutOfOrderRepliesCannotReopenAnApplication() {
        val tracker = NetworkSettingsApplicationTracker()
        val token = tracker.begin(3)
        assertNull(tracker.runtimeReplied(token, 3))
        assertFalse(tracker.committed(token, 2, "old-session"))
        assertTrue(tracker.committed(token, 3, "saved"))
        assertFalse(tracker.committed(token, 3, "duplicate"))
        assertNotNull(tracker.runtimeReplied(token, 3))
        assertNull(tracker.runtimeReplied(token, 3))
        assertEquals("saved", tracker.current(3)?.operationId)
        assertNotNull(tracker.finish(token, 3))
        assertNull(tracker.runtimeReplied(token, 3))
        assertFalse(tracker.committed(token, 3, "late"))
        assertNull(tracker.finish(token, 3))
        assertFalse(tracker.busy)
    }

    @Test
    fun cancellationRejectsLateCallbacksEvenWhenTheNextSaveUsesTheSameSession() {
        // All explicit cancellation boundaries use cancel(), including manual
        // connection, disconnect, data clearing, and service destruction.
        val tracker = NetworkSettingsApplicationTracker()
        val retired = tracker.begin(4)
        assertTrue(tracker.committed(retired, 4, "retired"))
        tracker.cancel()
        assertFalse(tracker.isLatest(retired))
        val next = tracker.begin(4)
        assertFalse(tracker.committed(retired, 4, "late-save"))
        assertNull(tracker.runtimeReplied(retired, 4))
        assertNull(tracker.finish(retired, 4))
        assertFalse(tracker.migrateSession(retired, 4, 5))
        assertTrue(tracker.owns(next, 4))
        assertFalse(tracker.allowsObservation(4))
        assertEquals(NetworkSettingsApplicationTracker.Phase.PERSISTING, tracker.phase)
    }

    @Test
    fun onlyAControlledReconfigurationCanCarryItsTokenIntoTheNextSession() {
        val tracker = NetworkSettingsApplicationTracker()
        val token = tracker.begin(10)
        assertFalse(tracker.migrateSession(token, 10, 11))
        assertTrue(tracker.committed(token, 10, "cold-save"))
        assertFalse(tracker.migrateSession(token + 1, 10, 11))
        assertTrue(tracker.migrateSession(token, 10, 11))
        tracker.cancelIfStale(11)
        assertTrue(tracker.owns(token, 11))
        assertEquals("cold-save", tracker.current(11)?.operationId)
        assertFalse(tracker.allowsObservation(11))
        assertNull(tracker.runtimeReplied(token, 10))
        assertNull(tracker.finish(token, 10))
        assertNotNull(tracker.runtimeReplied(token, 11))
        assertFalse(tracker.migrateSession(token, 11, 12))
        assertTrue(tracker.allowsObservation(11))
        val completed = requireNotNull(tracker.finish(token, 11))
        assertEquals("cold-save", completed.operationId)
        assertEquals(11L, completed.generation)
        assertFalse(tracker.busy)
    }

    @Test
    fun anUncontrolledGenerationChangeRetiresTheReservationBeforeTheNextSave() {
        val tracker = NetworkSettingsApplicationTracker()
        val token = tracker.begin(20)
        tracker.cancelIfStale(20)
        assertTrue(tracker.owns(token, 20))
        assertFalse(tracker.allowsObservation(21))
        tracker.cancelIfStale(21)
        assertFalse(tracker.busy)
        assertFalse(tracker.isLatest(token))
        val next = tracker.begin(21)
        assertNull(tracker.finish(token, 20))
        assertFalse(tracker.committed(token, 20, "late-save"))
        assertTrue(tracker.owns(next, 21))
        assertFalse(tracker.allowsObservation(21))
    }

    @Test
    fun completedResultsCanPublishOnlyUntilANewerReservationOrCancellation() {
        val tracker = NetworkSettingsApplicationTracker()
        val first = tracker.begin(1)
        assertNotNull(tracker.finish(first, 1))
        assertTrue(tracker.isLatest(first))
        val second = tracker.begin(1)
        assertFalse(tracker.isLatest(first))
        assertTrue(tracker.isLatest(second))
        assertNull(tracker.finish(first, 1))
        assertTrue(tracker.owns(second, 1))
        assertNotNull(tracker.finish(second, 1))
        tracker.cancel()
        assertFalse(tracker.isLatest(second))
    }
}
