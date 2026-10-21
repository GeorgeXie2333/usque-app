package io.github.georgexie2333.usque

/** Main-thread owner of session replacement after physical changes or retryable network failures. */
internal class SessionNetworkRecovery(
    private val suspendSession: () -> Unit,
    private val stop: ((Boolean) -> Unit) -> Unit,
    private val schedule: (Long, () -> Unit) -> Unit,
    private val restart: () -> Unit,
    private val cleanupFailed: () -> Unit,
) {
    private var revision = 0L
    private var timerRevision = 0L
    private var networkGeneration = 0L
    private var lastObservedNetworkGeneration = Long.MIN_VALUE
    private var networkWake = false
    private var stopping = false
    private var stopped = false
    private var connecting = false
    private var timerScheduled = false
    private var waitingForNetworkChange = false
    private var retryFailures = 0
    var active = false
        private set

    /** The caller permits initial activation only for a previously established session. */
    fun failed(
        retryable: Boolean,
        networkGeneration: Long,
        networkPresent: Boolean,
        waitForNetworkChange: Boolean = false,
    ): Boolean {
        lastObservedNetworkGeneration = maxOf(lastObservedNetworkGeneration, networkGeneration)
        if (!retryable) {
            cancel()
            return false
        }
        waitingForNetworkChange = waitForNetworkChange
        if (waitForNetworkChange) invalidateTimer()
        if (!active) {
            active = true
            this.networkGeneration = networkGeneration
            retryFailures = 0
            stopSession()
        } else {
            val newUsableNetwork = updateNetwork(networkGeneration, networkPresent)
            if (connecting) {
                if (!newUsableNetwork) retryFailures = (retryFailures + 1).coerceAtMost(1)
                stopSession()
            } else {
                scheduleRestart()
            }
        }
        return true
    }

    /** A new physical generation may rebuild an established chain; ordinary sessions recover natively. */
    fun networkChanged(
        networkGeneration: Long,
        networkPresent: Boolean,
        restartEstablishedSession: Boolean = false,
    ): Boolean {
        if (networkGeneration <= lastObservedNetworkGeneration) return active
        lastObservedNetworkGeneration = networkGeneration
        if (!active) {
            if (!restartEstablishedSession) return false
            return failed(
                retryable = true,
                networkGeneration = networkGeneration,
                networkPresent = networkPresent,
            )
        }
        if (networkGeneration <= this.networkGeneration) return true
        updateNetwork(networkGeneration, networkPresent)
        if (networkPresent) waitingForNetworkChange = false
        if (connecting) {
            // Revoke the startup worker before stopping native so a late success
            // cannot publish a session on a superseded physical network.
            stopSession()
        } else {
            scheduleRestart()
        }
        return true
    }

    private fun updateNetwork(
        generation: Long,
        present: Boolean,
    ): Boolean {
        if (generation <= networkGeneration) return false
        networkGeneration = generation
        networkWake = present
        invalidateTimer()
        return present
    }

    private fun stopSession() {
        if (stopping) return
        invalidateTimer()
        stopped = false
        connecting = false
        stopping = true
        val owner = ++revision
        // The service invalidates its startup worker and snapshots, while
        // retaining the TUN until confirmed native cleanup permits replacement.
        suspendSession()
        stop { confirmed ->
            if (!active || revision != owner || !stopping) return@stop
            stopping = false
            if (!confirmed) {
                cancel()
                cleanupFailed()
            } else {
                stopped = true
                scheduleRestart()
            }
        }
    }

    private fun scheduleRestart() {
        if (!active || !stopped || stopping || connecting || timerScheduled ||
            waitingForNetworkChange
        ) {
            return
        }
        val owner = revision
        val timer = ++timerRevision
        timerScheduled = true
        val delay =
            if (networkWake) {
                NETWORK_SETTLE_MILLIS
            } else if (retryFailures == 0) {
                0L
            } else {
                RETRY_INTERVAL_MILLIS
            }
        networkWake = false
        schedule(delay) {
            if (!active || revision != owner || timerRevision != timer || !stopped || connecting) {
                return@schedule
            }
            timerScheduled = false
            stopped = false
            connecting = true
            restart()
        }
    }

    private fun invalidateTimer() {
        timerRevision++
        timerScheduled = false
    }

    fun connected() {
        cancel()
    }

    /** Manual connection, disconnect, terminal failure and destruction revoke all pending work. */
    fun cancel() {
        revision++
        invalidateTimer()
        active = false
        stopping = false
        stopped = false
        connecting = false
        waitingForNetworkChange = false
        retryFailures = 0
        networkWake = false
        // Keep consumed generations: a delayed duplicate physical callback
        // cannot reauthorize a canceled or successfully replaced session.
    }

    private companion object {
        const val RETRY_INTERVAL_MILLIS = 1_000L
        const val NETWORK_SETTLE_MILLIS = 100L
    }
}
