package io.github.georgexie2333.usque

/** Captures the retiring runtime before recovery invalidates its generation and snapshot. */
internal fun loggedSessionNetworkRecovery(
    captureLogContext: () -> ServiceLogContext,
    suspendSession: () -> Unit,
    stop: (ServiceLogContext, (Boolean) -> Unit) -> Unit,
    schedule: (Long, () -> Unit) -> Unit,
    restart: () -> Unit,
    cleanupFailed: () -> Unit,
): SessionNetworkRecovery {
    var retiringContext: ServiceLogContext? = null
    return SessionNetworkRecovery(
        suspendSession = {
            retiringContext = captureLogContext()
            suspendSession()
        },
        stop = { completed ->
            val context = checkNotNull(retiringContext)
            retiringContext = null
            // Each stop keeps its immutable scope through asynchronous cleanup,
            // even if a manual command starts another recovery meanwhile.
            stop(context, completed)
        },
        schedule = schedule,
        restart = restart,
        cleanupFailed = cleanupFailed,
    )
}
