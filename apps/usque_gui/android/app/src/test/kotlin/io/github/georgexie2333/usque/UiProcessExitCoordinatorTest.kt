package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.CountDownLatch

class UiProcessExitCoordinatorTest {
    private class Scheduled(
        val delay: Long,
        val action: () -> Unit,
        var cancelled: Boolean = false,
        var delivered: Boolean = false,
    )

    private class Harness {
        val scheduled = mutableListOf<Scheduled>()
        var exits = 0
        val coordinator =
            UiProcessExitCoordinator(
                schedule = { delay, action ->
                    val task = Scheduled(delay, action)
                    scheduled.add(task)
                    UiExitCancellation { task.cancelled = true }
                },
                terminate = { exits++ },
            )

        fun create(): Any = Any().also(coordinator::activityCreated)

        fun finish(
            activity: Any,
            main: Boolean = true,
        ) {
            coordinator.activityDestroyed(activity, main, isFinishing = true, isChangingConfigurations = false)
        }

        fun runNext() {
            val task = scheduled.first { !it.cancelled && !it.delivered }
            task.delivered = true
            task.action()
        }

        fun pendingCount(): Int = scheduled.count { !it.cancelled && !it.delivered }
    }

    @Test
    fun finishingMainWaitsForDelayedCallbackBeforeExiting() {
        val h = Harness()
        h.finish(h.create())
        assertEquals(0, h.exits)
        assertEquals(1_000L, h.scheduled.single().delay)
        h.runNext()
        assertEquals(1, h.exits)
    }

    @Test
    fun homeAndPermissionDialogKeepTheActivityAlive() {
        val h = Harness()
        val activity = h.create()
        h.coordinator.activityStarted(activity)
        // onPause/onStop do not destroy the Activity or request process exit.
        assertEquals(0, h.pendingCount())
        assertEquals(0, h.exits)
    }

    @Test
    fun configurationReplacementDoesNotRequestExit() {
        val h = Harness()
        val old = h.create()
        h.coordinator.activityDestroyed(old, isMainActivity = true, isFinishing = true, isChangingConfigurations = true)
        assertEquals(0, h.pendingCount())
        h.create()
        assertEquals(0, h.exits)
    }

    @Test
    fun nonFinishingDestructionDoesNotRequestExit() {
        val h = Harness()
        h.coordinator.activityDestroyed(
            h.create(),
            isMainActivity = true,
            isFinishing = false,
            isChangingConfigurations = false,
        )
        assertEquals(0, h.pendingCount())
    }

    @Test
    fun finishingOtherActivityAloneDoesNotRequestExit() {
        val h = Harness()
        h.finish(h.create(), main = false)
        assertEquals(0, h.pendingCount())
        assertEquals(0, h.exits)
    }

    @Test
    fun finishingMainWaitsForEveryOtherActivityToBeDestroyed() {
        val h = Harness()
        val main = h.create()
        val secondMain = h.create()
        val other = h.create()
        h.finish(main)
        h.finish(secondMain)
        assertEquals(0, h.pendingCount())
        h.finish(other, main = false)
        h.runNext()
        assertEquals(1, h.exits)
    }

    @Test
    fun newActivityRevokesPreviousExitEvenIfCancelledCallbackArrives() {
        val h = Harness()
        h.finish(h.create())
        val stale = h.scheduled.single()
        val newActivity = h.create()
        assertTrue(stale.cancelled)
        stale.action()
        assertEquals(0, h.exits)
        h.finish(newActivity, main = false)
        assertEquals(0, h.pendingCount())
        assertEquals(0, h.exits)
    }

    @Test
    fun activityRestartAlsoRevokesPreviousExit() {
        val h = Harness()
        h.finish(h.create())
        val stale = h.scheduled.single()
        val existingActivity = Any()
        h.coordinator.activityStarted(existingActivity)
        stale.action()
        h.finish(existingActivity, main = false)
        assertEquals(0, h.pendingCount())
        assertEquals(0, h.exits)
    }

    @Test
    fun reopenedMainCanRequestFreshExitWithoutAcceptingStaleCallback() {
        val h = Harness()
        h.finish(h.create())
        val stale = h.scheduled.single()
        h.finish(h.create())
        stale.action()
        assertEquals(0, h.exits)
        assertEquals(1, h.pendingCount())
        h.runNext()
        assertEquals(1, h.exits)
    }

    @Test
    fun cleanupWaitsForActualCompletionWithoutTimeoutKill() {
        val h = Harness()
        var terminated = false
        h.coordinator.awaitCleanup { terminated }
        h.finish(h.create())
        repeat(20) {
            h.runNext()
            assertEquals(0, h.exits)
            assertEquals(1, h.pendingCount())
            assertEquals(1_000L, h.scheduled.last().delay)
        }
        terminated = true
        h.runNext()
        assertEquals(1, h.exits)
    }

    @Test
    fun configurationReplacementCannotDiscardOldExecutorCleanup() {
        val h = Harness()
        val old = h.create()
        var oldTerminated = false
        var newTerminated = false
        h.coordinator.awaitCleanup { oldTerminated }
        h.coordinator.activityDestroyed(
            old,
            isMainActivity = true,
            isFinishing = false,
            isChangingConfigurations = true,
        )
        val replacement = h.create()
        h.coordinator.awaitCleanup { newTerminated }
        h.finish(replacement)
        newTerminated = true
        h.runNext()
        assertEquals(0, h.exits)
        oldTerminated = true
        h.runNext()
        assertEquals(1, h.exits)
    }

    @Test
    fun unknownCleanupStateFailsClosed() {
        val h = Harness()
        h.coordinator.awaitCleanup { throw IllegalStateException("Unavailable") }
        h.finish(h.create())
        h.runNext()
        assertEquals(0, h.exits)
        assertEquals(1, h.pendingCount())
    }

    @Test
    fun workLeaseSuspendsPollingAndReleaseResumesDelayedExit() {
        val h = Harness()
        val lease = h.coordinator.holdBackgroundWork()
        h.finish(h.create())
        assertEquals(0, h.pendingCount())
        lease.close()
        assertEquals(0, h.exits)
        h.runNext()
        assertEquals(1, h.exits)
    }

    @Test
    fun lateWorkLeaseInvalidatesAlreadyQueuedExit() {
        val h = Harness()
        h.finish(h.create())
        val stale = h.scheduled.single()
        val lease = h.coordinator.holdBackgroundWork()
        stale.action()
        assertEquals(0, h.exits)
        assertEquals(0, h.pendingCount())
        lease.close()
        h.runNext()
        assertEquals(1, h.exits)
    }

    @Test
    fun repeatedCloseCannotReleaseAnotherWorkLease() {
        val h = Harness()
        val first = h.coordinator.holdBackgroundWork()
        val second = h.coordinator.holdBackgroundWork()
        h.finish(h.create())
        first.close()
        first.close()
        assertEquals(0, h.pendingCount())
        second.close()
        second.close()
        assertEquals(1, h.pendingCount())
        h.runNext()
        assertEquals(1, h.exits)
    }

    @Test
    fun concurrentClosesReleaseLeaseOnlyOnce() {
        val h = Harness()
        val lease = h.coordinator.holdBackgroundWork()
        val blocker = h.coordinator.holdBackgroundWork()
        h.finish(h.create())
        val ready = CountDownLatch(1)
        val threads =
            List(8) {
                Thread {
                    ready.await()
                    lease.close()
                }.apply { start() }
            }
        ready.countDown()
        threads.forEach { it.join() }
        assertEquals(0, h.pendingCount())
        blocker.close()
        h.runNext()
        assertEquals(1, h.exits)
    }

    @Test
    fun backgroundWorkWithoutFinishedMainNeverRequestsExit() {
        val h = Harness()
        h.coordinator.holdBackgroundWork().close()
        assertEquals(0, h.pendingCount())
    }

    @Test
    fun unknownOrDuplicateDestroyedActivityCannotAuthorizeExit() {
        val h = Harness()
        h.finish(Any())
        assertEquals(0, h.pendingCount())
        val other = h.create()
        h.finish(other, main = false)
        h.finish(other)
        assertEquals(0, h.pendingCount())
    }

    @Test
    fun processGuardOnlyAcceptsExactDefaultProcessIncludingValidationId() {
        val applicationId = "io.github.georgexie2333.usque"
        assertTrue(isUiProcess(applicationId, applicationId))
        assertTrue(isUiProcess("$applicationId.validation", "$applicationId.validation"))
        assertFalse(isUiProcess(applicationId, "$applicationId:vpn"))
        assertFalse(isUiProcess(applicationId, "$applicationId:other"))
        assertFalse(isUiProcess(applicationId, "$applicationId.validation"))
        assertFalse(isUiProcess(applicationId, null))
        assertFalse(isUiProcess(applicationId, ""))
        assertFalse(isUiProcess("", ""))
    }
}
