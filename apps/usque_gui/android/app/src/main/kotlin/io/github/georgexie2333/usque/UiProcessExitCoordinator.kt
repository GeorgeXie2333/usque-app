package io.github.georgexie2333.usque

import java.util.Collections
import java.util.IdentityHashMap

internal fun interface UiExitCancellation {
    fun cancel()
}

internal fun isUiProcess(
    packageName: String,
    processName: String?,
): Boolean = packageName.isNotEmpty() && processName == packageName

/**
 * Finishing the UI can leave a cached process while the separate VPN process runs.
 * All lifecycle events and delayed callbacks run on the main thread; work leases
 * can be acquired or released from any thread.
 */
internal class UiProcessExitCoordinator(
    private val schedule: (Long, () -> Unit) -> UiExitCancellation,
    private val terminate: () -> Unit,
) {
    private val activities = Collections.newSetFromMap(IdentityHashMap<Any, Boolean>())
    private val pendingCleanup = mutableListOf<() -> Boolean>()
    private var backgroundWork = 0
    private var exitRequested = false
    private var exited = false
    private var generation = 0L
    private var delayedExit: UiExitCancellation? = null

    @Synchronized
    fun activityCreated(activity: Any) {
        activities.add(activity)
        cancelExitRequest()
    }

    @Synchronized
    fun activityStarted(activity: Any) {
        activities.add(activity)
        cancelExitRequest()
    }

    @Synchronized
    fun activityDestroyed(
        activity: Any,
        isMainActivity: Boolean,
        isFinishing: Boolean,
        isChangingConfigurations: Boolean,
    ) {
        if (!activities.remove(activity)) return
        if (isMainActivity && isFinishing && !isChangingConfigurations) {
            exitRequested = true
        }
        scheduleIfReady()
    }

    /** Retain cleanup from every old Activity, including configuration replacements. */
    @Synchronized
    fun awaitCleanup(isComplete: () -> Boolean) {
        pendingCleanup.add(isComplete)
        collectCompletedCleanup()
        scheduleIfReady()
    }

    @Synchronized
    fun holdBackgroundWork(): AutoCloseable {
        backgroundWork++
        cancelDelayedExit()
        var closed = false
        return AutoCloseable {
            synchronized(this) {
                if (!closed) {
                    closed = true
                    backgroundWork--
                    scheduleIfReady()
                }
            }
        }
    }

    private fun cancelExitRequest() {
        exitRequested = false
        cancelDelayedExit()
        collectCompletedCleanup()
    }

    private fun cancelDelayedExit() {
        generation++
        delayedExit?.cancel()
        delayedExit = null
    }

    private fun collectCompletedCleanup() {
        pendingCleanup.removeAll { isComplete ->
            try {
                isComplete()
            } catch (_: RuntimeException) {
                // An unknown cleanup state must not authorize process termination.
                false
            }
        }
    }

    private fun scheduleIfReady() {
        if (exited || !exitRequested || activities.isNotEmpty() || backgroundWork != 0 || delayedExit != null) return
        val scheduledGeneration = generation
        delayedExit = schedule(EXIT_DELAY_MS) { attemptExit(scheduledGeneration) }
    }

    @Synchronized
    private fun attemptExit(scheduledGeneration: Long) {
        if (scheduledGeneration != generation || exited) return
        delayedExit = null
        if (!exitRequested || activities.isNotEmpty() || backgroundWork != 0) return
        collectCompletedCleanup()
        if (pendingCleanup.isNotEmpty()) {
            // Executor shutdown does not mean work has completed. Wait without
            // imposing a timeout on work that can still be persisting state.
            scheduleIfReady()
            return
        }
        exited = true
        terminate()
    }

    private companion object {
        const val EXIT_DELAY_MS = 1_000L
    }
}
