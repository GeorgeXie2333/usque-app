package io.github.georgexie2333.usque

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NativeStopTrackerTest {
    @Test
    fun queuedAndUnconfirmedStopsRemainPendingUntilConfirmed() {
        val tracker = NativeStopTracker()
        assertFalse(tracker.pendingCleanup())
        val first = tracker.begin()
        val second = tracker.begin()
        tracker.complete(first, true)
        assertTrue(tracker.pendingCleanup())
        tracker.complete(second, false)
        assertTrue(tracker.pendingCleanup())
        tracker.complete(tracker.begin(), true)
        assertFalse(tracker.pendingCleanup())
    }

    @Test
    fun oldCompletionCannotClearNewerUnconfirmedStop() {
        val tracker = NativeStopTracker()
        val first = tracker.begin()
        val second = tracker.begin()
        tracker.complete(second, false)
        tracker.complete(first, true)
        assertTrue(tracker.pendingCleanup())
    }
}
